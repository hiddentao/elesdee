use crate::state::{Account, Auth, Client};

#[derive(Clone, Copy, Debug)]
pub struct ProviderDef {
    pub id: &'static str,
    pub label: &'static str,
    pub client: Client,
    pub auth: &'static [Auth],
    pub base_url: Option<&'static str>,
    pub default_model: Option<&'static str>,
    pub custom: bool,
}

const KEY_ONLY: &[Auth] = &[Auth::Key];
const SUBSCRIPTION_OR_KEY: &[Auth] = &[Auth::Subscription, Auth::Key];
const KEY_OR_LOCAL: &[Auth] = &[Auth::Key, Auth::Local];

pub const PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        id: "anthropic",
        label: "Anthropic",
        client: Client::ClaudeCode,
        auth: SUBSCRIPTION_OR_KEY,
        base_url: None,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "openai",
        label: "OpenAI",
        client: Client::Codex,
        auth: SUBSCRIPTION_OR_KEY,
        base_url: None,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "google",
        label: "Google",
        client: Client::Antigravity,
        auth: KEY_ONLY,
        base_url: None,
        default_model: Some(crate::antigravity::DEFAULT_MODEL),
        custom: false,
    },
    ProviderDef {
        id: "deepseek",
        label: "DeepSeek",
        client: Client::ClaudeCode,
        auth: KEY_ONLY,
        base_url: Some("https://api.deepseek.com/anthropic"),
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "kimi",
        label: "Kimi (Moonshot)",
        client: Client::ClaudeCode,
        auth: KEY_ONLY,
        base_url: Some("https://api.moonshot.ai/anthropic"),
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "zai",
        label: "Z.ai",
        client: Client::ClaudeCode,
        auth: KEY_ONLY,
        base_url: Some("https://api.z.ai/api/anthropic"),
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "openrouter",
        label: "OpenRouter",
        client: Client::OpenCode2,
        auth: KEY_ONLY,
        base_url: None,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "ollama",
        label: "Ollama",
        client: Client::ClaudeCode,
        auth: KEY_OR_LOCAL,
        base_url: None,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "lmstudio",
        label: "LM Studio",
        client: Client::ClaudeCode,
        auth: KEY_OR_LOCAL,
        base_url: None,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "custom",
        label: "Custom provider",
        client: Client::OpenCode2,
        auth: KEY_OR_LOCAL,
        base_url: None,
        default_model: None,
        custom: true,
    },
];

pub fn find(id: &str) -> Option<&'static ProviderDef> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

pub fn clients_for_style(style: &str) -> &'static [Client] {
    match style {
        "openai" => &[Client::Codex, Client::OpenCode2],
        "anthropic" => &[Client::ClaudeCode, Client::OpenCode2],
        _ => &[],
    }
}

pub fn environment(account: &Account, key: Option<&str>) -> Vec<(String, String)> {
    let mut environment = Vec::new();
    let base = account.server.clone().or_else(|| {
        find(&account.provider).and_then(|provider| provider.base_url.map(str::to_owned))
    });
    let model = account.model.clone();

    let mut add = |name: &str, value: String| {
        if !value.is_empty() {
            environment.push((name.to_owned(), value));
        }
    };

    if account.provider == "custom" {
        match account.api_style.as_deref() {
            Some("openai") => {
                if let Some(base) = base {
                    add("OPENAI_BASE_URL", base);
                }
                if let Some(key) = key {
                    add("OPENAI_API_KEY", key.to_owned());
                }
                if let Some(model) = model {
                    add("OPENAI_MODEL", model);
                }
            }
            Some("anthropic") => {
                if let Some(base) = base {
                    add("ANTHROPIC_BASE_URL", base);
                }
                if let Some(key) = key {
                    add("ANTHROPIC_AUTH_TOKEN", key.to_owned());
                }
                if let Some(model) = model {
                    add("ANTHROPIC_MODEL", model);
                }
            }
            _ => {}
        }
        return environment;
    }

    match account.provider.as_str() {
        "anthropic" => {
            if let Some(key) = key {
                add("ANTHROPIC_API_KEY", key.to_owned());
            }
        }
        "openai" => {
            if let Some(key) = key {
                add("OPENAI_API_KEY", key.to_owned());
            }
        }
        "deepseek" | "kimi" | "zai" | "ollama" | "lmstudio" => {
            if let Some(base) = base {
                add("ANTHROPIC_BASE_URL", base);
            }
            match key {
                Some(key) => add("ANTHROPIC_AUTH_TOKEN", key.to_owned()),
                None if matches!(account.auth, Auth::Local) => {
                    add("ANTHROPIC_AUTH_TOKEN", "local".to_owned());
                }
                None => {}
            }
            if let Some(model) = model {
                add("ANTHROPIC_MODEL", model);
            }
        }
        "openrouter" => {
            if let Some(key) = key {
                add("OPENROUTER_API_KEY", key.to_owned());
            }
            if let Some(model) = model {
                add("OPENCODE_MODEL", model);
            }
        }
        _ => {}
    }

    environment
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageState {
    Unknown,
    Room,
    NearLimit,
}

pub fn usage_state(account: &Account) -> UsageState {
    let mut known = false;
    let mut near = false;
    for row in &account.rows {
        if let Some(used) = row.used {
            known = true;
            if used >= 90 {
                near = true;
            }
            continue;
        }
        if let (Some(value), Some(min)) = (
            row.value.as_deref().and_then(parse_number),
            row.min.as_deref().and_then(parse_number),
        ) {
            known = true;
            if value < min {
                near = true;
            }
        }
    }

    if near {
        UsageState::NearLimit
    } else if known {
        UsageState::Room
    } else {
        UsageState::Unknown
    }
}

fn parse_number(value: &str) -> Option<f64> {
    let cleaned = value.replace(',', "");
    let start = cleaned.find(|character: char| character.is_ascii_digit() || character == '-')?;
    let number = cleaned[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit() || matches!(character, '.' | '-'))
        .collect::<String>();
    number.parse().ok()
}

pub fn remaining_percent(account: &Account) -> Option<u8> {
    account
        .rows
        .iter()
        .filter_map(|row| row.used)
        .map(|used| 100u8.saturating_sub(used))
        .min()
}

/// The accounts a session may use when the launch does not name a list.
///
/// The default keeps the session on its starting client, so an automatic
/// switch continues the same conversation. The starting account is always
/// included, even when it is a sample.
pub fn default_allowed_accounts(
    accounts: &[Account],
    client: Option<Client>,
    account_id: Option<&str>,
) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(client) = client {
        for account in accounts {
            if !account.sample && account.client == client {
                ids.push(account.id.clone());
            }
        }
    }
    if let Some(account_id) = account_id
        && !ids.iter().any(|id| id == account_id)
    {
        ids.push(account_id.to_owned());
    }
    ids
}

fn subscription_rank(account: &Account) -> u8 {
    if account.auth == Auth::Subscription {
        1
    } else {
        0
    }
}

fn balance_above_min(account: &Account) -> Option<i64> {
    account
        .rows
        .iter()
        .filter_map(|row| {
            let value = row.value.as_deref().and_then(parse_number)?;
            let min = row.min.as_deref().and_then(parse_number)?;
            Some(((value - min) * 100.0).round() as i64)
        })
        .min()
}

/// The room left in an account, for ordering candidates.
///
/// Plan limits use the smallest remaining percentage. A prepaid balance uses
/// the smallest amount above its minimum. The two values never mix for one
/// account.
pub fn remaining_room(account: &Account) -> (u8, i64) {
    (
        remaining_percent(account).unwrap_or(0),
        balance_above_min(account).unwrap_or(0),
    )
}

/// Picks the account the automatic switch moves to next.
///
/// It skips samples, the current account and accounts without room, and it
/// stays inside the session's allowed list. Subscription accounts come first,
/// then pay-per-use accounts. Within a group the account with the most room
/// wins. A tie keeps the session on its current client, then falls back to the
/// account id for a stable result.
pub fn auto_switch_target(
    accounts: &[Account],
    allowed: &[String],
    current_client: Option<Client>,
    exclude_id: Option<&str>,
) -> Option<Account> {
    let mut candidates = accounts
        .iter()
        .filter(|account| {
            !account.sample
                && usage_state(account) == UsageState::Room
                && allowed.iter().any(|id| id == &account.id)
                && exclude_id != Some(account.id.as_str())
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|a, b| {
        subscription_rank(b)
            .cmp(&subscription_rank(a))
            .then_with(|| remaining_room(b).cmp(&remaining_room(a)))
            .then_with(|| {
                let same_a = current_client == Some(a.client);
                let same_b = current_client == Some(b.client);
                same_b.cmp(&same_a)
            })
            .then_with(|| a.id.cmp(&b.id))
    });

    candidates.first().map(|account| (*account).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UsageRow;

    fn account(provider: &str, client: Client, auth: Auth) -> Account {
        Account {
            id: "a".to_owned(),
            name: "Account".to_owned(),
            plan: "test".to_owned(),
            client,
            provider: provider.to_owned(),
            auth,
            sample: false,
            rows: Vec::new(),
            foot: String::new(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            server: None,
            model: None,
            api_style: None,
        }
    }

    #[test]
    fn provider_table_matches_the_supported_clients() {
        assert_eq!(
            find("anthropic").expect("anthropic").client,
            Client::ClaudeCode
        );
        assert_eq!(find("openai").expect("openai").client, Client::Codex);
        assert_eq!(find("google").expect("google").client, Client::Antigravity);
        assert_eq!(
            find("openrouter").expect("openrouter").client,
            Client::OpenCode2
        );
        assert!(find("unknown").is_none());
        assert_eq!(
            clients_for_style("anthropic"),
            &[Client::ClaudeCode, Client::OpenCode2]
        );
        assert_eq!(
            clients_for_style("openai"),
            &[Client::Codex, Client::OpenCode2]
        );
    }

    #[test]
    fn auto_switch_target_prefers_a_subscription_over_a_roomier_key_account() {
        let mut current = account("anthropic", Client::ClaudeCode, Auth::Subscription);
        current.id = "current".to_owned();
        let mut roomy = account("deepseek", Client::ClaudeCode, Auth::Key);
        roomy.id = "roomy".to_owned();
        roomy.rows = vec![UsageRow {
            label: "Weekly".to_owned(),
            used: Some(10),
            value: None,
            min: None,
            warn: None,
            window: None,
            resets_at: None,
        }];
        let mut tighter = account("anthropic", Client::ClaudeCode, Auth::Subscription);
        tighter.id = "tighter".to_owned();
        tighter.rows = vec![UsageRow {
            label: "Weekly".to_owned(),
            used: Some(60),
            value: None,
            min: None,
            warn: None,
            window: None,
            resets_at: None,
        }];
        let mut sample = account("anthropic", Client::ClaudeCode, Auth::Subscription);
        sample.id = "sample".to_owned();
        sample.sample = true;
        sample.rows = vec![UsageRow {
            label: "Weekly".to_owned(),
            used: Some(5),
            value: None,
            min: None,
            warn: None,
            window: None,
            resets_at: None,
        }];
        let mut other_client = account("openai", Client::Codex, Auth::Subscription);
        other_client.id = "codex".to_owned();
        other_client.rows = vec![UsageRow {
            label: "Weekly".to_owned(),
            used: Some(5),
            value: None,
            min: None,
            warn: None,
            window: None,
            resets_at: None,
        }];
        let mut unknown = account("deepseek", Client::ClaudeCode, Auth::Key);
        unknown.id = "unknown".to_owned();

        let accounts = vec![current, roomy, tighter, sample, other_client, unknown];
        let allowed = vec![
            "current".to_owned(),
            "roomy".to_owned(),
            "tighter".to_owned(),
        ];
        let target = auto_switch_target(
            &accounts,
            &allowed,
            Some(Client::ClaudeCode),
            Some("current"),
        )
        .expect("a target account");
        assert_eq!(target.id, "tighter");

        let all = vec![
            "current".to_owned(),
            "roomy".to_owned(),
            "tighter".to_owned(),
            "codex".to_owned(),
        ];
        let target = auto_switch_target(&accounts, &all, Some(Client::ClaudeCode), Some("current"))
            .expect("a target account");
        assert_eq!(
            target.id, "codex",
            "the subscription with the most room wins"
        );

        assert!(
            auto_switch_target(
                &accounts,
                &["codex".to_owned()],
                Some(Client::Codex),
                Some("codex")
            )
            .is_none()
        );
    }

    #[test]
    fn key_and_base_url_environment_follows_the_provider() {
        let deepseek = account("deepseek", Client::ClaudeCode, Auth::Key);
        let values = environment(&deepseek, Some("sk-test"));
        assert!(values.contains(&(
            "ANTHROPIC_BASE_URL".to_owned(),
            "https://api.deepseek.com/anthropic".to_owned()
        )));
        assert!(values.contains(&("ANTHROPIC_AUTH_TOKEN".to_owned(), "sk-test".to_owned())));

        let openai = account("openai", Client::Codex, Auth::Key);
        assert_eq!(
            environment(&openai, Some("sk-openai")),
            vec![("OPENAI_API_KEY".to_owned(), "sk-openai".to_owned())]
        );

        let local = account("ollama", Client::ClaudeCode, Auth::Local);
        let mut local = local;
        local.server = Some("http://127.0.0.1:11434".to_owned());
        let values = environment(&local, None);
        assert!(values.contains(&(
            "ANTHROPIC_BASE_URL".to_owned(),
            "http://127.0.0.1:11434".to_owned()
        )));
        assert!(values.contains(&("ANTHROPIC_AUTH_TOKEN".to_owned(), "local".to_owned())));
    }

    #[test]
    fn custom_provider_uses_its_api_style() {
        let mut custom = account("custom", Client::Codex, Auth::Key);
        custom.api_style = Some("openai".to_owned());
        custom.server = Some("https://gateway.example/v1".to_owned());
        custom.model = Some("gpt-test".to_owned());
        let values = environment(&custom, Some("sk-custom"));
        assert!(values.contains(&(
            "OPENAI_BASE_URL".to_owned(),
            "https://gateway.example/v1".to_owned()
        )));
        assert!(values.contains(&("OPENAI_API_KEY".to_owned(), "sk-custom".to_owned())));
        assert!(values.contains(&("OPENAI_MODEL".to_owned(), "gpt-test".to_owned())));
    }

    fn with_used(mut account: Account, used: u8) -> Account {
        account.rows = vec![crate::state::UsageRow {
            label: "Weekly".to_owned(),
            window: None,
            used: Some(used),
            value: None,
            min: None,
            warn: None,
            resets_at: None,
        }];
        account
    }

    fn with_balance(mut account: Account, value: &str, min: &str) -> Account {
        account.rows = vec![crate::state::UsageRow {
            label: "Balance".to_owned(),
            window: None,
            used: None,
            value: Some(value.to_owned()),
            min: Some(min.to_owned()),
            warn: None,
            resets_at: None,
        }];
        account
    }

    #[test]
    fn default_allowed_accounts_keep_the_starting_client() {
        let mut claude = account("anthropic", Client::ClaudeCode, Auth::Subscription);
        claude.id = "claude".to_owned();
        claude.sample = false;
        let mut deepseek = account("deepseek", Client::ClaudeCode, Auth::Key);
        deepseek.id = "deepseek".to_owned();
        deepseek.sample = false;
        let mut codex = account("openai", Client::Codex, Auth::Subscription);
        codex.id = "codex".to_owned();
        codex.sample = false;
        let mut sample = account("anthropic", Client::ClaudeCode, Auth::Subscription);
        sample.id = "sample".to_owned();
        sample.sample = true;
        let accounts = vec![claude, deepseek, codex, sample];

        assert_eq!(
            default_allowed_accounts(&accounts, Some(Client::ClaudeCode), Some("claude")),
            vec!["claude".to_owned(), "deepseek".to_owned()]
        );
        assert_eq!(
            default_allowed_accounts(&accounts, Some(Client::ClaudeCode), Some("sample")),
            vec![
                "claude".to_owned(),
                "deepseek".to_owned(),
                "sample".to_owned()
            ]
        );
        assert_eq!(
            default_allowed_accounts(&accounts, None, None),
            Vec::<String>::new()
        );
    }

    #[test]
    fn auto_switch_prefers_subscriptions_then_the_most_room() {
        let mut claude = with_used(
            account("anthropic", Client::ClaudeCode, Auth::Subscription),
            95,
        );
        claude.id = "claude".to_owned();
        claude.sample = false;
        let mut codex = with_used(account("openai", Client::Codex, Auth::Subscription), 50);
        codex.id = "codex".to_owned();
        codex.sample = false;
        let mut deepseek = with_used(account("deepseek", Client::ClaudeCode, Auth::Key), 5);
        deepseek.id = "deepseek".to_owned();
        deepseek.sample = false;
        let accounts = vec![claude.clone(), codex, deepseek];

        let allowed = vec![
            "claude".to_owned(),
            "codex".to_owned(),
            "deepseek".to_owned(),
        ];
        let target = auto_switch_target(
            &accounts,
            &allowed,
            Some(Client::ClaudeCode),
            Some("claude"),
        )
        .expect("a target");
        assert_eq!(target.id, "codex", "a subscription account wins");
    }

    #[test]
    fn auto_switch_breaks_a_tie_toward_the_current_client() {
        let mut claude = with_used(
            account("anthropic", Client::ClaudeCode, Auth::Subscription),
            95,
        );
        claude.id = "claude".to_owned();
        claude.sample = false;
        let mut same = with_used(
            account("anthropic", Client::ClaudeCode, Auth::Subscription),
            60,
        );
        same.id = "same-client".to_owned();
        same.sample = false;
        let mut other = with_used(account("openai", Client::Codex, Auth::Subscription), 60);
        other.id = "other-client".to_owned();
        other.sample = false;
        let accounts = vec![claude, same, other];

        let allowed = vec!["same-client".to_owned(), "other-client".to_owned()];
        let target = auto_switch_target(&accounts, &allowed, Some(Client::ClaudeCode), None)
            .expect("a target");
        assert_eq!(target.id, "same-client");
    }

    #[test]
    fn auto_switch_never_leaves_the_allowed_list() {
        let mut claude = with_used(
            account("anthropic", Client::ClaudeCode, Auth::Subscription),
            95,
        );
        claude.id = "claude".to_owned();
        claude.sample = false;
        let mut backup = with_used(account("anthropic", Client::ClaudeCode, Auth::Key), 10);
        backup.id = "backup".to_owned();
        backup.sample = false;
        let accounts = vec![claude.clone(), backup];

        let allowed = vec!["claude".to_owned()];
        assert!(
            auto_switch_target(
                &accounts,
                &allowed,
                Some(Client::ClaudeCode),
                Some("claude"),
            )
            .is_none()
        );
    }

    #[test]
    fn auto_switch_orders_prepaid_balances_by_the_amount_above_the_minimum() {
        let mut claude = with_used(
            account("anthropic", Client::ClaudeCode, Auth::Subscription),
            95,
        );
        claude.id = "claude".to_owned();
        claude.sample = false;
        let mut low = with_balance(
            account("deepseek", Client::ClaudeCode, Auth::Key),
            "7.00",
            "5.00",
        );
        low.id = "low".to_owned();
        low.sample = false;
        let mut high = with_balance(
            account("deepseek", Client::ClaudeCode, Auth::Key),
            "20.00",
            "5.00",
        );
        high.id = "high".to_owned();
        high.sample = false;
        let accounts = vec![claude, low, high];

        let allowed = vec!["low".to_owned(), "high".to_owned()];
        let target = auto_switch_target(&accounts, &allowed, Some(Client::ClaudeCode), None)
            .expect("a target");
        assert_eq!(target.id, "high");
    }
}
