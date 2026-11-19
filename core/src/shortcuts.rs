use crate::state::{Client, Shortcut};

pub fn for_client(client: Client) -> Vec<Shortcut> {
    match client {
        Client::ClaudeCode => vec![
            shortcut("Shift+Tab", "⇧Tab", "Shift Tab", "Cycle permission modes"),
            shortcut("Ctrl+O", "^O", "Control O", "Verbose transcript"),
            shortcut("Ctrl+T", "^T", "Control T", "To-do list"),
            shortcut("Ctrl+R", "^R", "Control R", "History search"),
            shortcut("Ctrl+G", "^G", "Control G", "External editor"),
        ],
        Client::Codex => vec![
            shortcut("Shift+Tab", "⇧Tab", "Shift Tab", "Cycle approval modes"),
            shortcut("Ctrl+T", "^T", "Control T", "Transcript overlay"),
            shortcut("Ctrl+R", "^R", "Control R", "History search"),
            shortcut("Ctrl+G", "^G", "Control G", "External editor"),
        ],
        Client::OpenCode2 => vec![
            shortcut("Ctrl+P", "^P", "Control P", "Command palette"),
            shortcut("Ctrl+L", "^L", "Control L", "Clear screen"),
        ],
        Client::Antigravity => Vec::new(),
    }
}

fn shortcut(key: &str, label: &str, name: &str, description: &str) -> Shortcut {
    Shortcut {
        key: key.to_owned(),
        label: label.to_owned(),
        name: name.to_owned(),
        description: description.to_owned(),
    }
}
