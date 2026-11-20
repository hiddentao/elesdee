//! Bounded console and network records for one session browser.
//!
//! The records are short and local to a session. Every entry is capped and
//! cleaned, and addresses lose their login details and sensitive query values.
//! A secret value that the app knows is removed later, when a tool returns the
//! text.

use std::collections::VecDeque;

pub const MAX_CONSOLE_ENTRIES: usize = 200;
pub const MAX_NETWORK_ENTRIES: usize = 200;
pub const MAX_LOG_TEXT_CHARS: usize = 1000;
pub const MAX_NETWORK_URL_CHARS: usize = 512;

const SENSITIVE_PARAMS: &[&str] = &[
    "access_token",
    "api_key",
    "apikey",
    "auth",
    "authorization",
    "client_secret",
    "code",
    "id_token",
    "key",
    "passwd",
    "password",
    "refresh_token",
    "session",
    "sessionid",
    "sig",
    "signature",
    "state",
    "token",
];

/// The console and network records of one session browser.
#[derive(Debug, Default)]
pub struct BrowserLogs {
    console: VecDeque<ConsoleEntry>,
    network: VecDeque<NetworkEntry>,
}

#[derive(Clone, Debug)]
pub struct ConsoleEntry {
    level: String,
    text: String,
}

#[derive(Clone, Debug)]
pub struct NetworkEntry {
    request_id: String,
    method: String,
    url: String,
    resource_type: String,
    status: Option<i64>,
    failure: Option<String>,
}

impl BrowserLogs {
    /// Removes every record, for example when the browser stops.
    pub fn clear(&mut self) {
        self.console.clear();
        self.network.clear();
    }

    /// Adds a console message. The oldest message leaves when the buffer is
    /// full.
    pub fn push_console(&mut self, level: &str, text: &str) {
        let text = cap(redact_text_urls(text), MAX_LOG_TEXT_CHARS);
        self.console.push_back(ConsoleEntry {
            level: clean(level).to_lowercase(),
            text,
        });
        while self.console.len() > MAX_CONSOLE_ENTRIES {
            self.console.pop_front();
        }
    }

    /// Adds a network request. A later response or failure fills in its result.
    pub fn push_network(&mut self, request_id: &str, method: &str, url: &str, resource_type: &str) {
        let entry = NetworkEntry {
            request_id: request_id.to_owned(),
            method: cap(clean(method).to_uppercase(), 16),
            url: redact_url(url),
            resource_type: clean(resource_type),
            status: None,
            failure: None,
        };
        self.network.push_back(entry);
        while self.network.len() > MAX_NETWORK_ENTRIES {
            self.network.pop_front();
        }
    }

    /// Records the response status of a request that is already in the buffer.
    pub fn finish_network(&mut self, request_id: &str, status: i64) {
        if let Some(entry) = self
            .network
            .iter_mut()
            .rev()
            .find(|entry| entry.request_id == request_id)
        {
            entry.status = Some(status);
            entry.failure = None;
        }
    }

    /// Records a failed request that is already in the buffer.
    pub fn fail_network(&mut self, request_id: &str, reason: &str) {
        if let Some(entry) = self
            .network
            .iter_mut()
            .rev()
            .find(|entry| entry.request_id == request_id)
        {
            entry.failure = Some(cap(clean(reason), MAX_LOG_TEXT_CHARS));
        }
    }

    /// Returns the last `limit` console messages, oldest first.
    pub fn console_text(&self, limit: usize) -> String {
        if self.console.is_empty() {
            return "The console has no messages.".to_owned();
        }
        let start = self.console.len().saturating_sub(limit.max(1));
        self.console
            .iter()
            .skip(start)
            .map(|entry| format!("[{}] {}", entry.level, entry.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Returns the last `limit` network requests, oldest first.
    pub fn network_text(&self, limit: usize) -> String {
        if self.network.is_empty() {
            return "The browser has no network requests.".to_owned();
        }
        let start = self.network.len().saturating_sub(limit.max(1));
        self.network
            .iter()
            .skip(start)
            .map(|entry| {
                let kind = if entry.resource_type.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", entry.resource_type)
                };
                match (&entry.status, &entry.failure) {
                    (_, Some(reason)) => {
                        format!("{} {}{kind} failed: {reason}", entry.method, entry.url)
                    }
                    (Some(status), None) => {
                        format!("{} {}{kind} returned {status}", entry.method, entry.url)
                    }
                    (None, None) => format!("{} {}{kind} did not finish", entry.method, entry.url),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Hides login details and sensitive query values, then caps the length.
pub fn redact_url(value: &str) -> String {
    let value = clean(value);
    let (base, query) = match value.split_once('?') {
        Some((before, after)) => (before.to_owned(), Some(after)),
        None => (value.clone(), None),
    };
    let mut output = redact_userinfo(&base);
    if let Some(query) = query {
        let (query, fragment) = match query.split_once('#') {
            Some((query, fragment)) => (query, Some(fragment)),
            None => (query, None),
        };
        output.push('?');
        output.push_str(&redact_pairs(query));
        if let Some(fragment) = fragment {
            output.push('#');
            output.push_str(&redact_pairs(fragment));
        }
    } else if let Some((path, fragment)) = output.split_once('#') {
        let redacted = format!("{path}#{}", redact_pairs(fragment));
        output = redacted;
    }
    cap(output, MAX_NETWORK_URL_CHARS)
}

/// Redacts every address inside a free-text console message.
fn redact_text_urls(value: &str) -> String {
    clean(value)
        .split_whitespace()
        .map(|token| {
            if token.contains("://") {
                redact_url(token)
            } else {
                token.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_userinfo(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_owned();
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let Some((_, host)) = authority.rsplit_once('@') else {
        return value.to_owned();
    };
    format!("{scheme}://[redacted]@{host}{tail}")
}

fn redact_pairs(value: &str) -> String {
    value
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) if is_sensitive(name) => format!("{name}=[redacted]"),
            _ => pair.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn is_sensitive(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    SENSITIVE_PARAMS.contains(&name.as_str())
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn cap(value: String, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value;
    }
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_login_details_and_sensitive_query_values() {
        assert_eq!(
            redact_url("https://user:pass@example.com/path?q=1&token=abc&page=2#access_token=xyz"),
            "https://[redacted]@example.com/path?q=1&token=[redacted]&page=2#access_token=[redacted]"
        );
        assert_eq!(
            redact_url("https://example.com/callback?key=secret&value=ok"),
            "https://example.com/callback?key=[redacted]&value=ok"
        );
        assert_eq!(redact_url("data:text/plain,hello"), "data:text/plain,hello");
    }

    #[test]
    fn hides_login_details_inside_console_text() {
        let mut logs = BrowserLogs::default();
        logs.push_console(
            "error",
            "failed https://user:pass@example.com/path?token=abc&page=2 for the user",
        );

        let text = logs.console_text(10);
        assert!(!text.contains("user:pass"), "{text}");
        assert!(!text.contains("token=abc"), "{text}");
        assert!(text.contains("https://[redacted]@example.com/path?token=[redacted]&page=2"));
    }

    #[test]
    fn caps_entries_and_text() {
        let mut logs = BrowserLogs::default();
        for index in 0..(MAX_CONSOLE_ENTRIES + 25) {
            logs.push_console("log", &format!("message {index}"));
        }
        assert_eq!(logs.console.len(), MAX_CONSOLE_ENTRIES);
        let text = logs.console_text(MAX_CONSOLE_ENTRIES);
        assert!(text.starts_with("[log] message 25"), "{text}");
        assert!(text.ends_with("[log] message 224"), "{text}");

        let mut logs = BrowserLogs::default();
        logs.push_console("log", &"x".repeat(MAX_LOG_TEXT_CHARS + 50));
        let text = logs.console_text(10);
        assert_eq!(
            text.chars().count(),
            "log".chars().count() + 2 + 1 + MAX_LOG_TEXT_CHARS
        );
    }

    #[test]
    fn caps_network_entries_and_keeps_results() {
        let mut logs = BrowserLogs::default();
        for index in 0..(MAX_NETWORK_ENTRIES + 10) {
            logs.push_network(
                &format!("request-{index}"),
                "get",
                &format!("https://example.com/{index}?token=secret"),
                "Document",
            );
        }
        assert_eq!(logs.network.len(), MAX_NETWORK_ENTRIES);
        logs.finish_network(&format!("request-{}", MAX_NETWORK_ENTRIES + 5), 200);
        logs.fail_network(
            &format!("request-{}", MAX_NETWORK_ENTRIES + 6),
            "net::ERR_FAILED",
        );
        let text = logs.network_text(MAX_NETWORK_ENTRIES);
        assert!(!text.contains("secret"), "{text}");
        assert!(text.contains("https://example.com/"), "{text}");
        assert!(text.contains("returned 200"), "{text}");
        assert!(text.contains("failed: net::ERR_FAILED"), "{text}");
        assert!(!text.contains("https://example.com/0?"), "{text}");
    }

    #[test]
    fn empty_logs_have_a_clear_message() {
        let logs = BrowserLogs::default();
        assert_eq!(logs.console_text(10), "The console has no messages.");
        assert_eq!(
            logs.network_text(10),
            "The browser has no network requests."
        );
    }
}
