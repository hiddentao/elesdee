use std::env;
use std::path::PathBuf;

use crate::state::{Account, Auth, Client};

pub fn accounts() -> Vec<Account> {
    vec![
        Account {
            id: "max".to_owned(),
            name: "Anthropic Max 20×".to_owned(),
            plan: "work".to_owned(),
            client: Client::ClaudeCode,
            provider: "anthropic".to_owned(),
            auth: Auth::Subscription,
            sample: true,
            rows: Vec::new(),
            foot: "No official usage source.".to_owned(),
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
            sample: true,
            rows: Vec::new(),
            foot: "No official usage source.".to_owned(),
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
            sample: true,
            rows: Vec::new(),
            foot: "No official usage source.".to_owned(),
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
            sample: true,
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
