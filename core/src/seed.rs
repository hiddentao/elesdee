use std::env;
use std::path::PathBuf;

use crate::state::{Account, Auth, Client, Project, UsageRow};

pub fn projects() -> Vec<Project> {
    vec![
        Project {
            id: "web".to_owned(),
            name: "web".to_owned(),
            folder: expand_tilde("~/dev/elesdee/web")
                .to_string_lossy()
                .into_owned(),
        },
        Project {
            id: "core".to_owned(),
            name: "core".to_owned(),
            folder: expand_tilde("~/dev/elesdee/core")
                .to_string_lossy()
                .into_owned(),
        },
    ]
}

pub fn accounts() -> Vec<Account> {
    vec![
        Account {
            id: "max".to_owned(),
            name: "Anthropic Max 20×".to_owned(),
            plan: "work".to_owned(),
            client: Client::ClaudeCode,
            provider: "anthropic".to_owned(),
            auth: Auth::Subscription,
            rows: vec![
                UsageRow {
                    label: "5-hour".to_owned(),
                    used: Some(92),
                    value: None,
                    min: None,
                    warn: Some(true),
                },
                UsageRow {
                    label: "Weekly".to_owned(),
                    used: Some(61),
                    value: None,
                    min: None,
                    warn: None,
                },
            ],
            foot: "Resets in 2h 10m. Read 1 min ago.".to_owned(),
            server: None,
            model: None,
        },
        Account {
            id: "codex".to_owned(),
            name: "ChatGPT Pro".to_owned(),
            plan: "Codex".to_owned(),
            client: Client::Codex,
            provider: "openai".to_owned(),
            auth: Auth::Subscription,
            rows: vec![UsageRow {
                label: "Weekly".to_owned(),
                used: Some(78),
                value: None,
                min: None,
                warn: None,
            }],
            foot: "No five-hour limit on this plan. Read 2 min ago.".to_owned(),
            server: None,
            model: None,
        },
        Account {
            id: "deepseek".to_owned(),
            name: "DeepSeek API".to_owned(),
            plan: "pay as you go".to_owned(),
            client: Client::ClaudeCode,
            provider: "deepseek".to_owned(),
            auth: Auth::Key,
            rows: vec![UsageRow {
                label: "Balance".to_owned(),
                used: None,
                value: Some("$12.40".to_owned()),
                min: Some("minimum $5.00".to_owned()),
                warn: None,
            }],
            foot: "Below the minimum, auto-switch moves the session away.".to_owned(),
            server: None,
            model: None,
        },
        Account {
            id: "litellm".to_owned(),
            name: "LiteLLM gateway".to_owned(),
            plan: "team".to_owned(),
            client: Client::Codex,
            provider: "custom".to_owned(),
            auth: Auth::Key,
            rows: Vec::new(),
            foot: "No official usage source.".to_owned(),
            server: Some("http://localhost:4000/v1".to_owned()),
            model: Some("gpt-5.2".to_owned()),
        },
    ]
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
    {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}
