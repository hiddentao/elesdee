use crate::state::{AiConfig, Auth, Client, Provider};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiStyle {
    OpenAi,
    Anthropic,
}

#[derive(Clone, Copy, Debug)]
pub struct ProviderDef {
    pub id: &'static str,
    pub label: &'static str,
    pub auth: &'static [Auth],
    pub clients: &'static [Client],
    pub default_model: Option<&'static str>,
    pub custom: bool,
}

const KEY_ONLY: &[Auth] = &[Auth::Key];
const SUBSCRIPTION_OR_KEY: &[Auth] = &[Auth::Subscription, Auth::Key];
const KEY_OR_LOCAL: &[Auth] = &[Auth::Key, Auth::Local];

const CLAUDE_AND_OPENCODE: &[Client] = &[Client::ClaudeCode, Client::OpenCode2];
const CODEX_AND_OPENCODE: &[Client] = &[Client::Codex, Client::OpenCode2];
const OPENCODE_AND_CODEX: &[Client] = &[Client::OpenCode2, Client::Codex];
const DEEPSEEK_CLIENTS: &[Client] = &[Client::ClaudeCode, Client::Codex, Client::OpenCode2];
const GOOGLE_CLIENTS: &[Client] = &[Client::Antigravity];
const LOCAL_CLIENTS: &[Client] = &[Client::ClaudeCode, Client::OpenCode2];

pub const PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        id: "anthropic",
        label: "Anthropic",
        auth: SUBSCRIPTION_OR_KEY,
        clients: CLAUDE_AND_OPENCODE,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "openai",
        label: "OpenAI",
        auth: SUBSCRIPTION_OR_KEY,
        clients: CODEX_AND_OPENCODE,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "google",
        label: "Google",
        auth: KEY_ONLY,
        clients: GOOGLE_CLIENTS,
        default_model: Some(crate::antigravity::DEFAULT_MODEL),
        custom: false,
    },
    ProviderDef {
        id: "deepseek",
        label: "DeepSeek",
        auth: KEY_ONLY,
        clients: DEEPSEEK_CLIENTS,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "kimi",
        label: "Kimi (Moonshot)",
        auth: KEY_ONLY,
        clients: CLAUDE_AND_OPENCODE,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "zai",
        label: "Z.ai",
        auth: KEY_ONLY,
        clients: CLAUDE_AND_OPENCODE,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "openrouter",
        label: "OpenRouter",
        auth: KEY_ONLY,
        clients: OPENCODE_AND_CODEX,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "ollama",
        label: "Ollama",
        auth: KEY_OR_LOCAL,
        clients: LOCAL_CLIENTS,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "lmstudio",
        label: "LM Studio",
        auth: KEY_OR_LOCAL,
        clients: LOCAL_CLIENTS,
        default_model: None,
        custom: false,
    },
    ProviderDef {
        id: "custom",
        label: "Custom provider",
        auth: KEY_OR_LOCAL,
        clients: CLAUDE_AND_OPENCODE,
        default_model: None,
        custom: true,
    },
];

pub fn find(id: &str) -> Option<&'static ProviderDef> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

/// The client that owns the subscription login for a vendor.
///
/// Anthropic runs its login through Claude Code and OpenAI through Codex.
/// The other vendors have no subscription sign-in.
pub fn sign_in_client(vendor: &str) -> Option<Client> {
    match vendor {
        "anthropic" => Some(Client::ClaudeCode),
        "openai" => Some(Client::Codex),
        _ => None,
    }
}

pub fn clients_for_style(style: &str) -> &'static [Client] {
    match style {
        "openai" => CODEX_AND_OPENCODE,
        "anthropic" => CLAUDE_AND_OPENCODE,
        _ => &[],
    }
}

/// The clients a stored provider can run.
///
/// A custom provider follows its API style. Every other vendor has a fixed
/// list, and DeepSeek runs on three clients.
pub fn supported_clients(provider: &Provider) -> Vec<Client> {
    if provider.vendor == "custom" {
        return provider
            .api_style
            .as_deref()
            .map(clients_for_style)
            .map(<[Client]>::to_vec)
            .unwrap_or_default();
    }
    find(&provider.vendor)
        .map(|definition| definition.clients.to_vec())
        .unwrap_or_default()
}

pub fn supports_client(provider: &Provider, client: Client) -> bool {
    supported_clients(provider).contains(&client)
}

/// The effort levels a client accepts, from low to high.
pub fn effort_levels(client: Client) -> &'static [&'static str] {
    match client {
        Client::ClaudeCode => &["low", "medium", "high", "xhigh", "max"],
        Client::Codex => &["minimal", "low", "medium", "high"],
        Client::OpenCode2 => &["low", "medium", "high"],
        Client::Antigravity => &["low", "medium", "high"],
    }
}

pub fn valid_effort(client: Client, effort: &str) -> bool {
    effort_levels(client).contains(&effort)
}

/// The model list to show when the provider read fails or the vendor has no
/// documented source.
pub fn known_models(vendor: &str) -> Vec<(String, String)> {
    let ids: &[&str] = match vendor {
        "anthropic" => &["claude-sonnet-4-5", "claude-opus-4-1", "claude-haiku-4-5"],
        "openai" => &["gpt-5.2", "gpt-5.1", "gpt-5-mini"],
        "google" => &[crate::antigravity::DEFAULT_MODEL, "gemini-3.8-pro-high"],
        "deepseek" => &["deepseek-chat", "deepseek-reasoner"],
        "kimi" => &["kimi-k2", "kimi-k2-turbo"],
        "zai" => &["glm-4.6", "glm-4.5"],
        "openrouter" => &["openai/gpt-5.2", "anthropic/claude-sonnet-4-5"],
        "ollama" => &["llama3.2", "qwen2.5-coder"],
        "lmstudio" => &["local-model"],
        _ => &[],
    };
    ids.iter()
        .map(|id| ((*id).to_owned(), label_for(id)))
        .collect()
}

fn label_for(id: &str) -> String {
    id.to_owned()
}

pub struct Endpoint {
    pub style: ApiStyle,
    pub base: String,
}

fn trimmed(base: &str) -> String {
    base.trim_end_matches('/').to_owned()
}

/// The API endpoint for one client, and the style it speaks.
///
/// A stored server address overrides the vendor default. The client chooses
/// between the two styles where a vendor offers both.
pub fn endpoint(provider: &Provider, client: Client) -> Option<Endpoint> {
    let server = provider
        .server
        .as_deref()
        .map(str::trim)
        .filter(|server| !server.is_empty());
    let custom_style = provider.api_style.as_deref();
    let style = match provider.vendor.as_str() {
        "custom" => match custom_style {
            Some("openai") => ApiStyle::OpenAi,
            Some("anthropic") => ApiStyle::Anthropic,
            _ if client == Client::ClaudeCode => ApiStyle::Anthropic,
            _ => ApiStyle::OpenAi,
        },
        "anthropic" | "kimi" | "zai" => {
            if client == Client::ClaudeCode {
                ApiStyle::Anthropic
            } else {
                ApiStyle::OpenAi
            }
        }
        "deepseek" => {
            if client == Client::ClaudeCode {
                ApiStyle::Anthropic
            } else {
                ApiStyle::OpenAi
            }
        }
        "ollama" | "lmstudio" => {
            if client == Client::ClaudeCode {
                ApiStyle::Anthropic
            } else {
                ApiStyle::OpenAi
            }
        }
        _ => ApiStyle::OpenAi,
    };

    let base = match provider.vendor.as_str() {
        "anthropic" => server
            .map(str::to_owned)
            .unwrap_or_else(|| "https://api.anthropic.com".to_owned()),
        "openai" => server
            .map(str::to_owned)
            .unwrap_or_else(|| "https://api.openai.com/v1".to_owned()),
        "google" => server.map(str::to_owned).unwrap_or_else(|| {
            "https://generativelanguage.googleapis.com/v1beta/openai".to_owned()
        }),
        "deepseek" => match server {
            Some(server) => server.to_owned(),
            None if style == ApiStyle::Anthropic => "https://api.deepseek.com/anthropic".to_owned(),
            None => "https://api.deepseek.com".to_owned(),
        },
        "kimi" => match server {
            Some(server) => server.to_owned(),
            None if style == ApiStyle::Anthropic => "https://api.moonshot.ai/anthropic".to_owned(),
            None => "https://api.moonshot.ai/v1".to_owned(),
        },
        "zai" => match server {
            Some(server) => server.to_owned(),
            None if style == ApiStyle::Anthropic => "https://api.z.ai/api/anthropic".to_owned(),
            None => "https://api.z.ai/api/paas/v4".to_owned(),
        },
        "openrouter" => server
            .map(str::to_owned)
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_owned()),
        "ollama" | "lmstudio" | "custom" => {
            let server = server?;
            if style == ApiStyle::OpenAi && !server.ends_with("/v1") {
                format!("{}/v1", server.trim_end_matches('/'))
            } else {
                server.to_owned()
            }
        }
        _ => server.map(str::to_owned)?,
    };

    Some(Endpoint {
        style,
        base: trimmed(&base),
    })
}

/// The environment that points a client at the provider.
///
/// Subscription providers need no key here, because the installed agent stores
/// its own credential.
pub fn environment(
    provider: &Provider,
    client: Client,
    key: Option<&str>,
) -> Vec<(String, String)> {
    if provider.vendor == crate::antigravity::PROVIDER_ID {
        return Vec::new();
    }
    let Some(endpoint) = endpoint(provider, client) else {
        return Vec::new();
    };
    let mut environment = Vec::new();
    let mut add = |name: &str, value: String| {
        if !value.is_empty() {
            environment.push((name.to_owned(), value));
        }
    };

    match endpoint.style {
        ApiStyle::Anthropic => {
            let direct = provider.vendor == "anthropic" && provider.server.is_none();
            if direct {
                if let Some(key) = key {
                    add("ANTHROPIC_API_KEY", key.to_owned());
                }
            } else {
                add("ANTHROPIC_BASE_URL", endpoint.base.clone());
                match key {
                    Some(key) => add("ANTHROPIC_AUTH_TOKEN", key.to_owned()),
                    None if provider.auth == Auth::Local => {
                        add("ANTHROPIC_AUTH_TOKEN", "local".to_owned());
                    }
                    None => {}
                }
            }
        }
        ApiStyle::OpenAi => {
            if !(provider.vendor == "openai" && provider.server.is_none()) {
                add("OPENAI_BASE_URL", endpoint.base.clone());
            }
            if let Some(key) = key {
                add("OPENAI_API_KEY", key.to_owned());
                if provider.vendor == "openrouter" {
                    add("OPENROUTER_API_KEY", key.to_owned());
                }
            }
        }
    }

    environment
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageState {
    Unknown,
    Room,
    NearLimit,
}

/// The default prepaid floor, in the currency the provider reports.
pub const DEFAULT_MINIMUM_BALANCE: f64 = 5.0;

/// Whether a vendor reports a prepaid balance that the user can floor.
pub fn vendor_has_balance(vendor: &str) -> bool {
    vendor == "deepseek"
}

pub fn usage_state(provider: &Provider) -> UsageState {
    if provider.limit_reported_at.is_some() {
        return UsageState::NearLimit;
    }
    let mut known = false;
    let mut near = false;
    for row in &provider.rows {
        if let Some(used) = row.used {
            known = true;
            if used >= 90 {
                near = true;
            }
            continue;
        }
        if let Some(value) = row.value.as_deref().and_then(parse_number)
            && let Some(minimum) = provider.minimum_balance
        {
            known = true;
            if value <= minimum {
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

pub fn remaining_percent(provider: &Provider) -> Option<u8> {
    provider
        .rows
        .iter()
        .filter_map(|row| row.used)
        .map(|used| 100u8.saturating_sub(used))
        .min()
}

/// The configs a session may use when the launch does not name a list.
///
/// The default keeps the session on its starting client, so an automatic
/// switch continues the same conversation. The starting config is always
/// included, even when its provider is a sample.
pub fn default_allowed_configs(
    configs: &[AiConfig],
    providers: &[Provider],
    client: Option<Client>,
    config_id: Option<&str>,
) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(client) = client {
        for config in configs {
            let sample = providers
                .iter()
                .find(|provider| provider.id == config.provider_id)
                .is_none_or(|provider| provider.sample);
            if !sample && config.client == client && !ids.iter().any(|id| id == &config.id) {
                ids.push(config.id.clone());
            }
        }
    }
    if let Some(config_id) = config_id
        && !ids.iter().any(|id| id == config_id)
    {
        ids.push(config_id.to_owned());
    }
    ids
}

fn subscription_rank(provider: Option<&Provider>) -> u8 {
    if provider.is_some_and(|provider| provider.auth == Auth::Subscription) {
        1
    } else {
        0
    }
}

fn balance_above_minimum(provider: &Provider) -> Option<i64> {
    let minimum = provider.minimum_balance?;
    provider
        .rows
        .iter()
        .filter(|row| row.used.is_none())
        .filter_map(|row| {
            let value = row.value.as_deref().and_then(parse_number)?;
            Some(((value - minimum) * 100.0).round() as i64)
        })
        .min()
}

/// The room left in a provider, for ordering candidates.
pub fn remaining_room(provider: &Provider) -> (u8, i64) {
    (
        remaining_percent(provider).unwrap_or(0),
        balance_above_minimum(provider).unwrap_or(0),
    )
}

/// Picks the config the automatic switch moves to next.
///
/// It skips samples, the current config and configs without room, and it stays
/// inside the session's allowed list. Subscription providers come first, then
/// pay-per-use providers. Within a group the provider with the most room wins.
/// A tie keeps the session on its current client, then falls back to the config
/// id for a stable result.
pub fn auto_switch_target(
    configs: &[AiConfig],
    providers: &[Provider],
    allowed: &[String],
    current_client: Option<Client>,
    exclude_id: Option<&str>,
) -> Option<AiConfig> {
    let mut candidates = configs
        .iter()
        .filter(|config| {
            allowed.iter().any(|id| id == &config.id) && exclude_id != Some(config.id.as_str())
        })
        .filter_map(|config| {
            let provider = providers
                .iter()
                .find(|provider| provider.id == config.provider_id)?;
            (!provider.sample && usage_state(provider) == UsageState::Room)
                .then_some((config, provider))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|(a, a_provider), (b, b_provider)| {
        subscription_rank(Some(b_provider))
            .cmp(&subscription_rank(Some(a_provider)))
            .then_with(|| remaining_room(b_provider).cmp(&remaining_room(a_provider)))
            .then_with(|| {
                let same_a = current_client == Some(a.client);
                let same_b = current_client == Some(b.client);
                same_b.cmp(&same_a)
            })
            .then_with(|| a.id.cmp(&b.id))
    });

    candidates.first().map(|(config, _)| (*config).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UsageRow;

    fn provider(vendor: &str, auth: Auth) -> Provider {
        Provider {
            id: "p".to_owned(),
            name: "Provider".to_owned(),
            vendor: vendor.to_owned(),
            auth,
            server: None,
            api_style: None,
            sample: false,
            connected: true,
            plan: "test".to_owned(),
            rows: Vec::new(),
            minimum_balance: None,
            foot: String::new(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            limit_reported_at: None,
        }
    }

    fn config_for(provider_id: &str, id: &str, client: Client) -> AiConfig {
        AiConfig {
            id: id.to_owned(),
            name: id.to_owned(),
            provider_id: provider_id.to_owned(),
            client,
            model: None,
            effort: None,
            args: Vec::new(),
            env: Vec::new(),
            config: Default::default(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn the_supported_clients_follow_the_vendor_table() {
        assert_eq!(
            clients_for_style("anthropic"),
            &[Client::ClaudeCode, Client::OpenCode2]
        );
        assert_eq!(
            clients_for_style("openai"),
            &[Client::Codex, Client::OpenCode2]
        );
        assert_eq!(
            supported_clients(&provider("deepseek", Auth::Key)),
            vec![Client::ClaudeCode, Client::Codex, Client::OpenCode2]
        );
        assert_eq!(
            supported_clients(&provider("google", Auth::Key)),
            vec![Client::Antigravity]
        );

        let mut custom = provider("custom", Auth::Key);
        custom.api_style = Some("anthropic".to_owned());
        assert_eq!(
            supported_clients(&custom),
            vec![Client::ClaudeCode, Client::OpenCode2]
        );
    }

    #[test]
    fn subscription_sign_in_names_its_client() {
        assert_eq!(sign_in_client("anthropic"), Some(Client::ClaudeCode));
        assert_eq!(sign_in_client("openai"), Some(Client::Codex));
        assert_eq!(sign_in_client("google"), None);
        assert_eq!(sign_in_client("deepseek"), None);
    }

    #[test]
    fn effort_lists_match_each_client() {
        assert_eq!(
            effort_levels(Client::ClaudeCode),
            &["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            effort_levels(Client::Codex),
            &["minimal", "low", "medium", "high"]
        );
        assert!(!effort_levels(Client::OpenCode2).is_empty());
        assert!(!effort_levels(Client::Antigravity).is_empty());
    }

    #[test]
    fn endpoints_pick_the_client_style() {
        let deepseek = provider("deepseek", Auth::Key);
        assert_eq!(
            endpoint(&deepseek, Client::ClaudeCode)
                .expect("endpoint")
                .base,
            "https://api.deepseek.com/anthropic"
        );
        assert_eq!(
            endpoint(&deepseek, Client::Codex).expect("endpoint").base,
            "https://api.deepseek.com"
        );

        let local = provider("ollama", Auth::Local);
        let mut local = local;
        local.server = Some("http://127.0.0.1:11434".to_owned());
        assert_eq!(
            endpoint(&local, Client::OpenCode2).expect("endpoint").base,
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            endpoint(&local, Client::ClaudeCode).expect("endpoint").base,
            "http://127.0.0.1:11434"
        );
    }

    #[test]
    fn environment_points_each_style_at_the_key() {
        let deepseek = provider("deepseek", Auth::Key);
        let anthropic = environment(&deepseek, Client::ClaudeCode, Some("sk-test"));
        assert!(anthropic.contains(&(
            "ANTHROPIC_BASE_URL".to_owned(),
            "https://api.deepseek.com/anthropic".to_owned()
        )));
        assert!(anthropic.contains(&("ANTHROPIC_AUTH_TOKEN".to_owned(), "sk-test".to_owned())));

        let openai = environment(&deepseek, Client::Codex, Some("sk-test"));
        assert!(openai.contains(&(
            "OPENAI_BASE_URL".to_owned(),
            "https://api.deepseek.com".to_owned()
        )));
        assert!(openai.contains(&("OPENAI_API_KEY".to_owned(), "sk-test".to_owned())));
    }

    #[test]
    fn a_subscription_provider_needs_no_key_environment() {
        let anthropic = provider("anthropic", Auth::Subscription);
        assert!(environment(&anthropic, Client::ClaudeCode, None).is_empty());
        let openai = provider("openai", Auth::Subscription);
        assert!(environment(&openai, Client::Codex, None).is_empty());
    }

    fn with_used(mut provider: Provider, used: u8) -> Provider {
        provider.rows = vec![UsageRow {
            label: "Weekly".to_owned(),
            window: None,
            used: Some(used),
            value: None,
            warn: None,
            resets_at: None,
        }];
        provider
    }

    fn with_balance(mut provider: Provider, value: &str, minimum: Option<f64>) -> Provider {
        provider.minimum_balance = minimum;
        provider.rows = vec![UsageRow {
            label: "Balance".to_owned(),
            window: Some("balance".to_owned()),
            used: None,
            value: Some(value.to_owned()),
            warn: None,
            resets_at: None,
        }];
        provider
    }

    #[test]
    fn a_balance_compares_against_the_provider_minimum() {
        let above = with_balance(provider("deepseek", Auth::Key), "USD 12.00", Some(5.0));
        assert_eq!(usage_state(&above), UsageState::Room);
        assert_eq!(remaining_room(&above).1, 700);

        assert_eq!(
            usage_state(&with_balance(
                provider("deepseek", Auth::Key),
                "USD 5.00",
                Some(5.0)
            )),
            UsageState::NearLimit
        );
        assert_eq!(
            usage_state(&with_balance(
                provider("deepseek", Auth::Key),
                "USD 4.99",
                Some(5.0)
            )),
            UsageState::NearLimit
        );
        assert_eq!(
            usage_state(&with_balance(
                provider("deepseek", Auth::Key),
                "USD 12.00",
                None
            )),
            UsageState::Unknown
        );
    }

    #[test]
    fn a_reported_limit_counts_as_near_the_limit() {
        let mut reported = with_used(provider("deepseek", Auth::Key), 5);
        assert_eq!(usage_state(&reported), UsageState::Room);
        reported.limit_reported_at = Some("2026-11-20T12:00:00Z".to_owned());
        assert_eq!(usage_state(&reported), UsageState::NearLimit);
    }

    #[test]
    fn auto_switch_prefers_a_subscription_with_room() {
        let mut claude = with_used(provider("anthropic", Auth::Subscription), 95);
        claude.id = "claude".to_owned();
        let mut codex = with_used(provider("openai", Auth::Subscription), 50);
        codex.id = "codex".to_owned();
        let mut deepseek = with_used(provider("deepseek", Auth::Key), 5);
        deepseek.id = "deepseek".to_owned();
        let providers = vec![claude, codex, deepseek];
        let configs = vec![
            config_for("claude", "claude", Client::ClaudeCode),
            config_for("codex", "codex", Client::Codex),
            config_for("deepseek", "deepseek", Client::ClaudeCode),
        ];
        let allowed = vec![
            "claude".to_owned(),
            "codex".to_owned(),
            "deepseek".to_owned(),
        ];
        let target = auto_switch_target(
            &configs,
            &providers,
            &allowed,
            Some(Client::ClaudeCode),
            Some("claude"),
        )
        .expect("a target");
        assert_eq!(target.id, "codex");
    }

    #[test]
    fn default_allowed_configs_keep_the_starting_client() {
        let mut anthropic = provider("anthropic", Auth::Subscription);
        anthropic.id = "anthropic".to_owned();
        let mut deepseek = provider("deepseek", Auth::Key);
        deepseek.id = "deepseek".to_owned();
        let mut openai = provider("openai", Auth::Subscription);
        openai.id = "openai".to_owned();
        let providers = vec![anthropic, deepseek, openai];
        let configs = vec![
            config_for("anthropic", "claude", Client::ClaudeCode),
            config_for("deepseek", "deepseek", Client::ClaudeCode),
            config_for("openai", "codex", Client::Codex),
        ];
        assert_eq!(
            default_allowed_configs(
                &configs,
                &providers,
                Some(Client::ClaudeCode),
                Some("claude")
            ),
            vec!["claude".to_owned(), "deepseek".to_owned()]
        );
    }
}
