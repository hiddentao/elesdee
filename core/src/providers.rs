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
    let base = account
        .server
        .clone()
        .or_else(|| find(&account.provider).and_then(|provider| provider.base_url.map(str::to_owned)));
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
    let start = cleaned
        .find(|character: char| character.is_ascii_digit() || character == '-')?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
            server: None,
            model: None,
            api_style: None,
        }
    }

    #[test]
    fn provider_table_matches_the_supported_clients() {
        assert_eq!(find("anthropic").expect("anthropic").client, Client::ClaudeCode);
        assert_eq!(find("openai").expect("openai").client, Client::Codex);
        assert_eq!(find("google").expect("google").client, Client::Antigravity);
        assert_eq!(find("openrouter").expect("openrouter").client, Client::OpenCode2);
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
}
