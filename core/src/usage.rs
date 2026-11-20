use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use serde_json::Value;

use crate::state::{AppState, Auth, Provider, UsageRow};

pub const FIVE_HOUR: &str = "five_hour";
pub const SEVEN_DAY: &str = "seven_day";
pub const BALANCE: &str = "balance";

pub const STATUS_LINE: &str = "statusline";
pub const APP_SERVER: &str = "app-server";
pub const DEEPSEEK: &str = "deepseek";
pub const MANUAL: &str = "manual";

const FIVE_HOUR_MINUTES: i64 = 5 * 60;
const SEVEN_DAY_MINUTES: i64 = 7 * 24 * 60;
const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const MIN_READ_INTERVAL: Duration = Duration::from_secs(60);
const COALESCE_WINDOW: Duration = Duration::from_secs(2);
const MIN_SAVE_INTERVAL: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ROWS: usize = 16;

#[derive(Debug)]
pub enum RefreshError {
    Unknown,
    Read,
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn window_row(window: &'static str, label: &'static str, value: &Value) -> Option<UsageRow> {
    let used = value.get("used_percentage")?.as_f64()?;
    let resets_at = value.get("resets_at").and_then(Value::as_i64);
    Some(UsageRow {
        label: label.to_owned(),
        window: Some(window.to_owned()),
        used: Some(used.round().clamp(0.0, 100.0) as u8),
        value: None,
        min: None,
        warn: None,
        resets_at,
    })
}

/// Reads the rate limits that Claude Code sends to a status line script.
pub fn claude_rows(payload: &Value) -> Vec<UsageRow> {
    let Some(limits) = payload.get("rate_limits") else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if let Some(window) = limits.get("five_hour")
        && let Some(row) = window_row(FIVE_HOUR, "5-hour limit", window)
    {
        rows.push(row);
    }
    if let Some(window) = limits.get("seven_day")
        && let Some(row) = window_row(SEVEN_DAY, "Weekly limit", window)
    {
        rows.push(row);
    }
    rows
}

fn codex_window(value: &Value) -> Option<(&'static str, &'static str, UsageRow)> {
    let minutes = value.get("windowDurationMins").and_then(Value::as_i64)?;
    let (window, label) = match minutes {
        FIVE_HOUR_MINUTES => (FIVE_HOUR, "5-hour limit"),
        SEVEN_DAY_MINUTES => (SEVEN_DAY, "Weekly limit"),
        _ => return None,
    };
    let used = value.get("usedPercent").and_then(Value::as_f64)?;
    let resets_at = value.get("resetsAt").and_then(Value::as_i64);
    Some((
        window,
        label,
        UsageRow {
            label: label.to_owned(),
            window: Some(window.to_owned()),
            used: Some(used.round().clamp(0.0, 100.0) as u8),
            value: None,
            min: None,
            warn: None,
            resets_at,
        },
    ))
}

/// Reads the rate limits from a Codex app-server read result or update
/// notification. The docs do not name the windows, so the window length
/// tells them apart.
pub fn codex_rows(payload: &Value) -> Vec<UsageRow> {
    let root = payload
        .get("result")
        .or_else(|| payload.get("params"))
        .unwrap_or(payload);
    let mut buckets = root
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .map(|map| map.values().collect::<Vec<_>>())
        .unwrap_or_default();
    if buckets.is_empty()
        && let Some(single) = root.get("rateLimits")
    {
        buckets.push(single);
    }
    if buckets.is_empty() && root.get("primary").is_some() {
        buckets.push(root);
    }

    let mut five_hour = None;
    let mut seven_day = None;
    for bucket in buckets {
        for window in ["primary", "secondary"] {
            let Some(value) = bucket.get(window) else {
                continue;
            };
            let Some((kind, _, row)) = codex_window(value) else {
                continue;
            };
            let slot = match kind {
                FIVE_HOUR => &mut five_hour,
                _ => &mut seven_day,
            };
            let keep = slot
                .as_ref()
                .is_none_or(|current: &UsageRow| row.used > current.used);
            if keep {
                *slot = Some(row);
            }
        }
    }

    [five_hour, seven_day].into_iter().flatten().collect()
}

fn clean_code(value: &str) -> Option<String> {
    let text = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>();
    (!text.is_empty()).then_some(text.to_ascii_uppercase())
}

fn clean_amount(value: &str) -> Option<String> {
    let text = value
        .chars()
        .filter(|character| {
            character.is_ascii_digit() || matches!(character, '.' | ',' | '-' | '+')
        })
        .take(32)
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}

/// Reads the balance from a DeepSeek `GET /user/balance` response.
pub fn deepseek_rows(payload: &Value) -> Vec<UsageRow> {
    let available = payload
        .get("is_available")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let Some(balances) = payload.get("balance_infos").and_then(Value::as_array) else {
        return Vec::new();
    };

    balances
        .iter()
        .filter_map(|balance| {
            let currency = clean_code(balance.get("currency")?.as_str()?)?;
            let total = clean_amount(balance.get("total_balance")?.as_str()?)?;
            Some(UsageRow {
                label: "Balance".to_owned(),
                window: Some(BALANCE.to_owned()),
                used: None,
                value: Some(format!("{currency} {total}")),
                min: None,
                warn: (!available).then_some(true),
                resets_at: None,
            })
        })
        .take(MAX_ROWS)
        .collect()
}

fn keep_minimum(rows: Vec<UsageRow>, previous: &[UsageRow]) -> Vec<UsageRow> {
    rows.into_iter()
        .map(|mut row| {
            let same_window = |old: &&UsageRow| old.window == row.window;
            let balance_without_kind = |old: &&UsageRow| {
                row.window.as_deref() == Some(BALANCE) && old.value.is_some() && old.used.is_none()
            };
            if let Some(old) = previous
                .iter()
                .find(same_window)
                .or_else(|| previous.iter().find(balance_without_kind))
                && row.min.is_none()
            {
                row.min = old.min.clone();
            }
            row
        })
        .collect()
}

/// Stores a fresh reading, tells the UI, saves at a bounded rate and runs
/// auto-switch against the new values.
pub async fn apply_usage(
    state: &AppState,
    provider_id: &str,
    source: &str,
    rows: Vec<UsageRow>,
) -> Option<Provider> {
    let previous = state.provider(provider_id)?;
    let rows = keep_minimum(rows, &previous.rows);
    let now = timestamp();
    let provider = state.set_provider_usage(provider_id, rows, source, &now, &now)?;
    state.publish_provider(provider.clone());
    if state.usage_save_due(provider_id, MIN_SAVE_INTERVAL) {
        state.note_usage_saved(provider_id);
        state.save();
    }
    crate::api::auto_switch_provider(state, &provider).await;
    Some(provider)
}

/// Marks a provider as out of usage after the agent reports a limit and runs
/// auto-switch at once, without waiting for the next usage poll.
pub async fn report_limit(state: &AppState, provider_id: &str) -> Option<Provider> {
    let provider = state.set_provider_limit_reported(provider_id, &timestamp())?;
    state.publish_provider(provider.clone());
    state.save();
    // The agent is waiting on this tool call. Let the reply return before the
    // switch restarts its process.
    let switch_state = state.clone();
    let switch_provider = provider.clone();
    tokio::spawn(async move {
        crate::api::auto_switch_provider(&switch_state, &switch_provider).await;
    });
    Some(provider)
}

/// Marks a failed read. The last successful values stay visible and keep
/// their read time; sessions are not touched.
pub fn mark_failed(state: &AppState, provider_id: &str, source: &str) -> Option<Provider> {
    let provider = state.mark_provider_usage_failed(provider_id, source, &timestamp())?;
    state.publish_provider(provider.clone());
    state.save();
    Some(provider)
}

async fn fetch_deepseek(base: &str, key: &str) -> Result<Value, ()> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| ())?;
    let url = format!("{}/user/balance", base.trim_end_matches('/'));
    let response = client
        .get(url)
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    response.json::<Value>().await.map_err(|_| ())
}

fn deepseek_base(state: &AppState, provider: &Provider) -> String {
    state
        .usage_base_override()
        .map(str::to_owned)
        .or_else(|| provider.server.clone())
        .unwrap_or_else(|| DEEPSEEK_BASE_URL.to_owned())
}

fn has_official_source(provider: &Provider) -> bool {
    provider.vendor == "deepseek" && provider.auth == Auth::Key
}

/// Reads one provider from its official source. Concurrent reads for the same
/// provider are serialized, and a recent read is reused.
pub async fn refresh_provider(
    state: &AppState,
    provider_id: &str,
    force: bool,
) -> Result<Provider, RefreshError> {
    let provider = state.provider(provider_id).ok_or(RefreshError::Unknown)?;
    if !has_official_source(&provider) {
        return Ok(provider);
    }

    let gate = state.usage_gate(provider_id);
    let _guard = gate.lock().await;
    let minimum = if force {
        COALESCE_WINDOW
    } else {
        MIN_READ_INTERVAL
    };
    if !state.usage_read_due(provider_id, minimum) {
        return state.provider(provider_id).ok_or(RefreshError::Unknown);
    }
    state.note_usage_attempt(provider_id);

    let key = match crate::api::account_key(state, provider_id).await {
        Ok(key) => key,
        Err(_) => {
            mark_failed(state, provider_id, DEEPSEEK);
            return Err(RefreshError::Read);
        }
    };
    let base = deepseek_base(state, &provider);
    let payload = match fetch_deepseek(&base, &key).await {
        Ok(payload) => payload,
        Err(()) => {
            tracing::warn!(provider = %provider_id, "the DeepSeek balance read failed");
            mark_failed(state, provider_id, DEEPSEEK);
            return Err(RefreshError::Read);
        }
    };
    let rows = deepseek_rows(&payload);
    if rows.is_empty() {
        tracing::warn!(provider = %provider_id, "the DeepSeek balance response had no balance");
        mark_failed(state, provider_id, DEEPSEEK);
        return Err(RefreshError::Read);
    }
    apply_usage(state, provider_id, DEEPSEEK, rows)
        .await
        .ok_or(RefreshError::Unknown)
}

/// Reads balances in the background. Each provider is read at most once a
/// minute, and only when it has a key and is not a sample.
pub fn start_poller(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(MIN_READ_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            for provider in state.providers() {
                if has_official_source(&provider) && !provider.sample {
                    let _ = refresh_provider(&state, &provider.id, false).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_status_line_reads_both_windows() {
        let payload = json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
                "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 }
            }
        });
        let rows = claude_rows(&payload);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].window.as_deref(), Some(FIVE_HOUR));
        assert_eq!(rows[0].used, Some(24));
        assert_eq!(rows[0].resets_at, Some(1738425600));
        assert_eq!(rows[1].window.as_deref(), Some(SEVEN_DAY));
        assert_eq!(rows[1].used, Some(41));
    }

    #[test]
    fn claude_status_line_accepts_missing_windows_and_values() {
        assert!(claude_rows(&json!({})).is_empty());
        assert!(claude_rows(&json!({ "rate_limits": {} })).is_empty());
        let rows = claude_rows(&json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 101.7, "resets_at": null },
                "seven_day": { "resets_at": 1 }
            }
        }));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].used, Some(100));
        assert_eq!(rows[0].resets_at, None);
    }

    #[test]
    fn codex_read_names_windows_by_length_and_keeps_the_tightest() {
        let payload = json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": { "usedPercent": 25, "windowDurationMins": 300, "resetsAt": 1730947200 },
                "secondary": { "usedPercent": 61, "windowDurationMins": 10080, "resetsAt": 1731552000 }
            },
            "rateLimitsByLimitId": {
                "codex": {
                    "primary": { "usedPercent": 25, "windowDurationMins": 300, "resetsAt": 1730947200 },
                    "secondary": { "usedPercent": 61, "windowDurationMins": 10080, "resetsAt": 1731552000 }
                },
                "codex_other": {
                    "primary": { "usedPercent": 87, "windowDurationMins": 300, "resetsAt": 1730947200 },
                    "secondary": null
                }
            }
        });
        let rows = codex_rows(&payload);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].window.as_deref(), Some(FIVE_HOUR));
        assert_eq!(rows[0].used, Some(87));
        assert_eq!(rows[1].window.as_deref(), Some(SEVEN_DAY));
        assert_eq!(rows[1].used, Some(61));
    }

    #[test]
    fn codex_read_accepts_a_notification_and_unknown_windows() {
        let notification = json!({
            "method": "account/rateLimits/updated",
            "params": {
                "rateLimits": {
                    "primary": { "usedPercent": 95, "windowDurationMins": 300, "resetsAt": 1 },
                    "secondary": { "usedPercent": 10, "windowDurationMins": 60, "resetsAt": 2 }
                }
            }
        });
        let rows = codex_rows(&notification);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].used, Some(95));
        assert!(codex_rows(&json!({ "result": {} })).is_empty());
        assert!(
            codex_rows(&json!({
                "rateLimits": {
                    "primary": { "usedPercent": 10, "windowDurationMins": 15 }
                }
            }))
            .is_empty()
        );

        let bare = codex_rows(&json!({
            "primary": { "usedPercent": 30, "windowDurationMins": 10080, "resetsAt": 3 }
        }));
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].window.as_deref(), Some(SEVEN_DAY));
        assert_eq!(bare[0].used, Some(30));
    }

    #[test]
    fn deepseek_balance_reads_currency_and_total() {
        let payload = json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "CNY",
                "total_balance": "110.00",
                "granted_balance": "10.00",
                "topped_up_balance": "100.00"
            }]
        });
        let rows = deepseek_rows(&payload);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].window.as_deref(), Some(BALANCE));
        assert_eq!(rows[0].value.as_deref(), Some("CNY 110.00"));
        assert_eq!(rows[0].warn, None);
    }

    #[test]
    fn deepseek_balance_marks_unavailable_and_cleans_text() {
        let payload = json!({
            "is_available": false,
            "balance_infos": [{
                "currency": "usd\n",
                "total_balance": "0.00<script>"
            }]
        });
        let rows = deepseek_rows(&payload);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value.as_deref(), Some("USD 0.00"));
        assert_eq!(rows[0].warn, Some(true));
        assert!(deepseek_rows(&json!({ "balance_infos": [] })).is_empty());
    }

    #[test]
    fn a_new_balance_keeps_the_minimum_the_user_set() {
        let previous = vec![UsageRow {
            label: "Balance".to_owned(),
            window: Some(BALANCE.to_owned()),
            used: None,
            value: Some("USD 9.00".to_owned()),
            min: Some("5".to_owned()),
            warn: Some(false),
            resets_at: None,
        }];
        let rows = keep_minimum(
            vec![UsageRow {
                label: "Balance".to_owned(),
                window: Some(BALANCE.to_owned()),
                used: None,
                value: Some("USD 12.00".to_owned()),
                min: None,
                warn: None,
                resets_at: None,
            }],
            &previous,
        );
        assert_eq!(rows[0].min.as_deref(), Some("5"));
    }

    #[test]
    fn a_balance_keeps_a_minimum_from_a_manual_row() {
        let previous = vec![UsageRow {
            label: "Manual balance".to_owned(),
            window: None,
            used: None,
            value: Some("USD 9.00".to_owned()),
            min: Some("5".to_owned()),
            warn: None,
            resets_at: None,
        }];
        let rows = keep_minimum(
            vec![UsageRow {
                label: "Balance".to_owned(),
                window: Some(BALANCE.to_owned()),
                used: None,
                value: Some("USD 12.00".to_owned()),
                min: None,
                warn: None,
                resets_at: None,
            }],
            &previous,
        );
        assert_eq!(rows[0].min.as_deref(), Some("5"));
    }
}
