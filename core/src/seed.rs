use std::env;
use std::path::PathBuf;

use crate::state::{AiConfig, Auth, Client, Provider};

const SAMPLE_FOOT: &str = "No official usage source.";

pub fn providers() -> Vec<Provider> {
    vec![
        Provider {
            id: "max".to_owned(),
            name: "Anthropic Max 20×".to_owned(),
            vendor: "anthropic".to_owned(),
            auth: Auth::Subscription,
            server: None,
            api_style: None,
            sample: true,
            connected: false,
            plan: "work".to_owned(),
            rows: Vec::new(),
            foot: SAMPLE_FOOT.to_owned(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            limit_reported_at: None,
        },
        Provider {
            id: "codex".to_owned(),
            name: "ChatGPT Pro".to_owned(),
            vendor: "openai".to_owned(),
            auth: Auth::Subscription,
            server: None,
            api_style: None,
            sample: true,
            connected: false,
            plan: "subscription".to_owned(),
            rows: Vec::new(),
            foot: SAMPLE_FOOT.to_owned(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            limit_reported_at: None,
        },
        Provider {
            id: "deepseek".to_owned(),
            name: "DeepSeek API".to_owned(),
            vendor: "deepseek".to_owned(),
            auth: Auth::Key,
            server: None,
            api_style: None,
            sample: true,
            connected: false,
            plan: "pay as you go".to_owned(),
            rows: Vec::new(),
            foot: SAMPLE_FOOT.to_owned(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            limit_reported_at: None,
        },
        Provider {
            id: "litellm".to_owned(),
            name: "LiteLLM gateway".to_owned(),
            vendor: "custom".to_owned(),
            auth: Auth::Key,
            server: Some("http://localhost:4000/v1".to_owned()),
            api_style: Some("openai".to_owned()),
            sample: true,
            connected: false,
            plan: "team".to_owned(),
            rows: Vec::new(),
            foot: SAMPLE_FOOT.to_owned(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            limit_reported_at: None,
        },
    ]
}

pub fn configs() -> Vec<AiConfig> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    vec![
        config(
            "max-claude-code",
            "Anthropic Max 20× (Claude Code)",
            "max",
            Client::ClaudeCode,
            None,
            &now,
        ),
        config(
            "codex-codex",
            "ChatGPT Pro (Codex)",
            "codex",
            Client::Codex,
            None,
            &now,
        ),
        config(
            "deepseek-claude-code",
            "DeepSeek API (Claude Code)",
            "deepseek",
            Client::ClaudeCode,
            None,
            &now,
        ),
        config(
            "litellm-codex",
            "LiteLLM gateway (Codex)",
            "litellm",
            Client::Codex,
            Some("gpt-5.2"),
            &now,
        ),
    ]
}

fn config(
    id: &str,
    name: &str,
    provider_id: &str,
    client: Client,
    model: Option<&str>,
    now: &str,
) -> AiConfig {
    AiConfig {
        id: id.to_owned(),
        name: name.to_owned(),
        provider_id: provider_id.to_owned(),
        client,
        model: model.map(str::to_owned),
        effort: None,
        args: Vec::new(),
        env: Vec::new(),
        config: std::collections::BTreeMap::new(),
        created_at: now.to_owned(),
        updated_at: now.to_owned(),
    }
}

pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(input));
    }
    if let Some(rest) = input
        .strip_prefix("~/")
        .or_else(|| input.strip_prefix("~\\"))
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(input)
}
