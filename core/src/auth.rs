//! Subscription sign-in.
//!
//! The user signs in through the installed client. This module starts the
//! client's own login command under a terminal, follows its output for a
//! sign-in address and a one-time code, and asks the client to confirm the
//! credential. The client keeps the credential. Nothing here reads a
//! credential file or stores a token.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Notify, mpsc};

use crate::state::{AppState, Auth, Client, Provider, ProviderView};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(15);
const EXIT_DRAIN_DELAY: Duration = Duration::from_millis(80);
const MAX_CODE_LENGTH: usize = 128;
const MAX_URL_LENGTH: usize = 2048;
const MAX_ATTEMPTS: usize = 16;

type LoginWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Per-process overrides for sign-in tests. The production path resolves the
/// installed client from the system path.
#[derive(Clone, Debug, Default)]
pub struct SignInOverrides {
    pub binary: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignInMode {
    #[default]
    Browser,
    Code,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInState {
    Starting,
    Waiting,
    Verifying,
    Success,
    Failed,
    Cancelled,
}

impl SignInState {
    fn is_active(self) -> bool {
        matches!(
            self,
            SignInState::Starting | SignInState::Waiting | SignInState::Verifying
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SignInReason {
    ClientNotFound,
    ClientError,
    NotSignedIn,
    NotSubscription,
    Timeout,
    Cancelled,
    VerificationFailed,
    Internal,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInRequest {
    pub vendor: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub mode: SignInMode,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInView {
    pub id: String,
    pub vendor: String,
    pub client: Client,
    pub mode: SignInMode,
    pub state: SignInState,
    pub url: Option<String>,
    pub code: Option<String>,
    pub reason: Option<SignInReason>,
    pub provider: Option<ProviderView>,
}

#[derive(Debug)]
pub enum SignInError {
    UnsupportedVendor,
    InvalidName,
    UnknownProvider,
    AlreadyRunning,
    UnknownAttempt,
    InvalidCode,
    NotWaiting,
    Internal,
}

/// The live sign-in attempts. Each attempt is shared with the task that
/// follows its client process.
#[derive(Clone, Default)]
pub struct SignInRegistry {
    attempts: Arc<Mutex<HashMap<String, Arc<Mutex<Attempt>>>>>,
}

impl SignInRegistry {
    fn insert(&self, attempt: Arc<Mutex<Attempt>>) {
        let mut attempts = lock(&self.attempts);
        attempts.retain(|_, existing| !lock(existing).state.is_terminal());
        if attempts.len() >= MAX_ATTEMPTS
            && let Some(oldest) = attempts
                .iter()
                .filter(|(_, existing)| lock(existing).state.is_terminal())
                .min_by_key(|(_, existing)| lock(existing).created_at)
                .map(|(id, _)| id.clone())
        {
            attempts.remove(&oldest);
        }
        let id = lock(&attempt).id.clone();
        attempts.insert(id, attempt);
    }

    fn find(&self, id: &str) -> Option<Arc<Mutex<Attempt>>> {
        lock(&self.attempts).get(id).cloned()
    }

    fn has_active(&self, vendor: &str, provider_id: Option<&str>) -> bool {
        lock(&self.attempts).values().any(|attempt| {
            let attempt = lock(attempt);
            attempt.state.is_active()
                && attempt.vendor == vendor
                && attempt.provider_id.as_deref() == provider_id
        })
    }
}

impl SignInState {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            SignInState::Success | SignInState::Failed | SignInState::Cancelled
        )
    }
}

struct Attempt {
    id: String,
    vendor: String,
    client: Client,
    mode: SignInMode,
    provider_id: Option<String>,
    name: Option<String>,
    state: SignInState,
    url: Option<String>,
    code: Option<String>,
    reason: Option<SignInReason>,
    provider: Option<ProviderView>,
    writer: Option<LoginWriter>,
    cancel: Arc<Notify>,
    created_at: std::time::Instant,
}

impl Attempt {
    fn view(&self) -> SignInView {
        SignInView {
            id: self.id.clone(),
            vendor: self.vendor.clone(),
            client: self.client,
            mode: self.mode,
            state: self.state,
            url: self.url.clone(),
            code: self.code.clone(),
            reason: self.reason,
            provider: self.provider.clone(),
        }
    }

    fn fail(&mut self, reason: SignInReason) {
        if !self.state.is_terminal() {
            self.state = SignInState::Failed;
            self.reason = Some(reason);
        }
    }
}

enum Signal {
    Url(String),
    Code(String),
    Enter,
}

enum Outcome {
    Success,
    Refused,
    Cancelled,
    Timeout,
}

/// Starts a subscription sign-in for an Anthropic or OpenAI provider.
pub async fn start(state: &AppState, request: SignInRequest) -> Result<SignInView, SignInError> {
    let vendor = request.vendor.trim().to_ascii_lowercase();
    let client = crate::providers::sign_in_client(&vendor).ok_or(SignInError::UnsupportedVendor)?;
    let name = validated_name(request.name.as_deref())?;
    let provider_id = request.provider_id;
    if let Some(id) = provider_id.as_deref() {
        let provider = state.provider(id).ok_or(SignInError::UnknownProvider)?;
        if provider.auth != Auth::Subscription
            || !crate::providers::supports_client(&provider, client)
        {
            return Err(SignInError::UnsupportedVendor);
        }
    }

    let registry = state.sign_ins();
    if registry.has_active(&vendor, provider_id.as_deref()) {
        return Err(SignInError::AlreadyRunning);
    }

    let attempt = Arc::new(Mutex::new(Attempt {
        id: uuid::Uuid::new_v4().simple().to_string(),
        vendor: vendor.clone(),
        client,
        mode: request.mode,
        provider_id,
        name,
        state: SignInState::Starting,
        url: None,
        code: None,
        reason: None,
        provider: None,
        writer: None,
        cancel: Arc::new(Notify::new()),
        created_at: std::time::Instant::now(),
    }));
    registry.insert(Arc::clone(&attempt));
    let view = lock(&attempt).view();

    let Some(binary) = sign_in_binary(state, client) else {
        lock(&attempt).fail(SignInReason::ClientNotFound);
        return Ok(view_now(&attempt));
    };

    let timeout = sign_in_timeout(state);
    let overrides = state.sign_in_overrides().unwrap_or_default();
    tokio::spawn(monitor(
        state.clone(),
        Arc::clone(&attempt),
        binary,
        overrides,
        timeout,
    ));
    Ok(view)
}

/// Returns the current state of a sign-in.
pub fn view(state: &AppState, id: &str) -> Option<SignInView> {
    let attempt = state.sign_ins().find(id)?;
    Some(lock(&attempt).view())
}

/// Sends a one-time code to the running login command.
pub async fn submit_code(
    state: &AppState,
    id: &str,
    code: &str,
) -> Result<SignInView, SignInError> {
    let code = code.trim();
    if code.is_empty()
        || code.chars().count() > MAX_CODE_LENGTH
        || code.chars().any(char::is_control)
    {
        return Err(SignInError::InvalidCode);
    }
    let attempt = state
        .sign_ins()
        .find(id)
        .ok_or(SignInError::UnknownAttempt)?;
    let writer = {
        let attempt = lock(&attempt);
        if attempt.state != SignInState::Waiting && attempt.state != SignInState::Starting {
            return Err(SignInError::NotWaiting);
        }
        attempt.writer.clone()
    };
    let Some(writer) = writer else {
        return Err(SignInError::NotWaiting);
    };

    let code = code.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut writer = lock(&writer);
        let _ = writer.write_all(code.as_bytes());
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
    })
    .await
    .map_err(|_| SignInError::Internal)?;

    Ok(lock(&attempt).view())
}

/// Ends a sign-in that the user abandoned.
pub fn cancel(state: &AppState, id: &str) -> Option<SignInView> {
    let attempt = state.sign_ins().find(id)?;
    let mut attempt = lock(&attempt);
    if !attempt.state.is_terminal() {
        attempt.state = SignInState::Cancelled;
        attempt.reason = Some(SignInReason::Cancelled);
        attempt.writer = None;
        attempt.cancel.notify_one();
    }
    Some(attempt.view())
}

fn view_now(attempt: &Arc<Mutex<Attempt>>) -> SignInView {
    lock(attempt).view()
}

fn validated_name(name: Option<&str>) -> Result<Option<String>, SignInError> {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    if name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(SignInError::InvalidName);
    }
    Ok(Some(name.to_owned()))
}

fn sign_in_binary(state: &AppState, client: Client) -> Option<PathBuf> {
    if let Some(overrides) = state.sign_in_overrides()
        && let Some(binary) = overrides.binary
    {
        return Some(binary);
    }
    if let Some(binary) = std::env::var_os("ELESDEE_SIGNIN_BIN")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Some(binary);
    }
    crate::clients::find_client(client)
}

fn sign_in_timeout(state: &AppState) -> Duration {
    state
        .sign_in_overrides()
        .and_then(|overrides| overrides.timeout)
        .or_else(|| {
            std::env::var("ELESDEE_SIGNIN_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.trim().parse::<u64>().ok())
                .map(Duration::from_secs)
        })
        .unwrap_or(DEFAULT_TIMEOUT)
}

fn login_arguments(client: Client, mode: SignInMode) -> Vec<&'static str> {
    match client {
        Client::ClaudeCode => vec!["auth", "login"],
        Client::Codex if mode == SignInMode::Code => vec!["login", "--device-auth"],
        Client::Codex => vec!["login"],
        Client::OpenCode2 | Client::Antigravity => Vec::new(),
    }
}

fn status_arguments(client: Client) -> Vec<&'static str> {
    match client {
        Client::ClaudeCode => vec!["auth", "status"],
        Client::Codex => vec!["login", "status"],
        Client::OpenCode2 | Client::Antigravity => Vec::new(),
    }
}

async fn monitor(
    state: AppState,
    attempt: Arc<Mutex<Attempt>>,
    binary: PathBuf,
    overrides: SignInOverrides,
    timeout: Duration,
) {
    let (client, mode, cancel) = {
        let attempt = lock(&attempt);
        (attempt.client, attempt.mode, Arc::clone(&attempt.cancel))
    };

    let mut command = CommandBuilder::new(&binary);
    command.env("NO_COLOR", "1");
    command.env("TERM", "xterm-256color");
    for (name, value) in &overrides.env {
        command.env(name, value);
    }
    for argument in login_arguments(client, mode) {
        command.arg(argument);
    }

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: 40,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(_) => {
            lock(&attempt).fail(SignInReason::Internal);
            return;
        }
    };
    let mut child = match pair.slave.spawn_command(command) {
        Ok(child) => child,
        Err(_) => {
            lock(&attempt).fail(SignInReason::ClientNotFound);
            return;
        }
    };
    let mut killer = child.clone_killer();
    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(_) => {
            let _ = killer.kill();
            lock(&attempt).fail(SignInReason::Internal);
            return;
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => Arc::new(Mutex::new(writer)),
        Err(_) => {
            let _ = killer.kill();
            lock(&attempt).fail(SignInReason::Internal);
            return;
        }
    };
    drop(pair.slave);

    lock(&attempt).writer = Some(Arc::clone(&writer));
    {
        let mut attempt = lock(&attempt);
        if attempt.state == SignInState::Starting {
            attempt.state = SignInState::Waiting;
        }
    }

    let (signal_tx, mut signals) = mpsc::unbounded_channel();
    let (exit_tx, mut exits) = mpsc::unbounded_channel();
    let want_code = client == Client::Codex && mode == SignInMode::Code;
    let _reader = thread::Builder::new()
        .name("sign-in-reader".to_owned())
        .spawn(move || read_output(reader, writer, want_code, signal_tx));
    let _waiter = thread::Builder::new()
        .name("sign-in-waiter".to_owned())
        .spawn(move || {
            let _ = exit_tx.send(child.wait().map(|status| status.success()).unwrap_or(false));
        });

    let outcome = loop {
        tokio::select! {
            signal = signals.recv() => {
                let Some(signal) = signal else { continue };
                apply_signal(&attempt, signal);
            }
            exit = exits.recv() => {
                tokio::time::sleep(EXIT_DRAIN_DELAY).await;
                while let Ok(signal) = signals.try_recv() {
                    apply_signal(&attempt, signal);
                }
                break if exit.unwrap_or(false) {
                    Outcome::Success
                } else {
                    Outcome::Refused
                };
            }
            _ = cancel.notified() => {
                let _ = killer.kill();
                break Outcome::Cancelled;
            }
            _ = tokio::time::sleep(timeout) => {
                let _ = killer.kill();
                break Outcome::Timeout;
            }
        }
    };

    lock(&attempt).writer = None;
    match outcome {
        Outcome::Cancelled => {
            let mut attempt = lock(&attempt);
            if !attempt.state.is_terminal() {
                attempt.state = SignInState::Cancelled;
                attempt.reason = Some(SignInReason::Cancelled);
            }
        }
        Outcome::Timeout => {
            let mut attempt = lock(&attempt);
            attempt.state = SignInState::Verifying;
            attempt.fail(SignInReason::Timeout);
        }
        Outcome::Refused => {
            lock(&attempt).fail(SignInReason::ClientError);
        }
        Outcome::Success => {
            {
                let mut attempt = lock(&attempt);
                if attempt.state.is_terminal() {
                    return;
                }
                attempt.state = SignInState::Verifying;
            }
            match verify(&binary, client, &overrides).await {
                Ok(()) if lock(&attempt).state.is_terminal() => {}
                Ok(()) => match connect(&state, &attempt) {
                    Ok(provider) => {
                        let mut attempt = lock(&attempt);
                        attempt.state = SignInState::Success;
                        attempt.reason = None;
                        attempt.provider = Some(ProviderView::new(provider));
                    }
                    Err(reason) => lock(&attempt).fail(reason),
                },
                Err(reason) => lock(&attempt).fail(reason),
            }
        }
    }
}

fn apply_signal(attempt: &Arc<Mutex<Attempt>>, signal: Signal) {
    let mut attempt = lock(attempt);
    match signal {
        Signal::Url(url) => {
            if attempt.url.is_none() {
                attempt.url = Some(url);
            }
        }
        Signal::Code(code) => {
            if attempt.code.is_none() {
                attempt.code = Some(code);
            }
        }
        Signal::Enter => {}
    }
}

fn read_output(
    mut reader: Box<dyn Read + Send>,
    writer: LoginWriter,
    want_code: bool,
    signals: mpsc::UnboundedSender<Signal>,
) {
    let mut buffer = [0_u8; 2048];
    let mut line = String::new();
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let text = String::from_utf8_lossy(&buffer[..count]);
        for character in text.chars() {
            if character == '\r' {
                continue;
            }
            if character == '\n' {
                emit_line(&line, want_code, &writer, &signals);
                line.clear();
            } else if line.len() < 4096 {
                line.push(character);
            }
        }
    }
    emit_line(&line, want_code, &writer, &signals);
}

fn emit_line(
    line: &str,
    want_code: bool,
    writer: &LoginWriter,
    signals: &mpsc::UnboundedSender<Signal>,
) {
    let line = strip_terminal_codes(line);
    if let Some(url) = find_url(&line) {
        let _ = signals.send(Signal::Url(url));
    }
    if want_code && let Some(code) = find_device_code(&line) {
        let _ = signals.send(Signal::Code(code));
    }
    if is_success_hint(&line) {
        let mut writer = lock(writer);
        let _ = writer.write_all(b"\r");
        let _ = writer.flush();
        let _ = signals.send(Signal::Enter);
    }
}

/// Removes terminal escape sequences so a line can be matched.
fn strip_terminal_codes(line: &str) -> String {
    let mut text = String::with_capacity(line.len());
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.next() == Some('[') {
                for code in characters.by_ref() {
                    if ('@'..='~').contains(&code) {
                        break;
                    }
                }
            }
            continue;
        }
        if !character.is_control() {
            text.push(character);
        }
    }
    text
}

fn find_url(line: &str) -> Option<String> {
    let start = line.find("https://").or_else(|| line.find("http://"))?;
    let rest = &line[start..];
    let end = rest
        .char_indices()
        .find(|(_, character)| {
            character.is_whitespace() || matches!(character, '"' | '\'' | '<' | '>' | ')')
        })
        .map(|(index, _)| index)
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ';', ':']);
    if url.len() > MAX_URL_LENGTH || url.len() <= "https://".len() {
        return None;
    }
    Some(url.to_owned())
}

/// Reads the short code that a device authorization flow prints.
fn find_device_code(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("code") || lower.contains("http") {
        return None;
    }
    line.split_whitespace()
        .map(|part| {
            part.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-'
            })
        })
        .find(|part| {
            let length = part.chars().count();
            let letters_are_upper = part
                .chars()
                .filter(|character| character.is_ascii_alphabetic())
                .all(|character| character.is_ascii_uppercase());
            (6..=16).contains(&length)
                && letters_are_upper
                && part
                    .chars()
                    .any(|character| character.is_ascii_alphabetic())
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
                && (part.contains('-') || part.chars().any(|character| character.is_ascii_digit()))
                && part.matches('-').count() <= 2
        })
        .map(str::to_owned)
}

fn is_success_hint(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("login successful")
        || lower.contains("successfully logged in")
        || lower.contains("logged in using")
        || lower.contains("already logged in")
}

async fn verify(
    binary: &Path,
    client: Client,
    overrides: &SignInOverrides,
) -> Result<(), SignInReason> {
    let arguments = status_arguments(client);
    if arguments.is_empty() {
        return Err(SignInReason::Internal);
    }
    let mut command = tokio::process::Command::new(binary);
    command
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    for (name, value) in &overrides.env {
        command.env(name, value);
    }

    let output = match tokio::time::timeout(VERIFY_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => return Err(SignInReason::ClientNotFound),
        Err(_) => return Err(SignInReason::VerificationFailed),
    };

    match client {
        Client::ClaudeCode => match claude_status(&output.stdout) {
            Some(true) => Ok(()),
            Some(false) => Err(SignInReason::NotSubscription),
            None if output.status.success() => Err(SignInReason::NotSignedIn),
            None => Err(SignInReason::NotSignedIn),
        },
        Client::Codex => {
            if output.status.success() {
                Ok(())
            } else {
                Err(SignInReason::NotSignedIn)
            }
        }
        Client::OpenCode2 | Client::Antigravity => Err(SignInReason::Internal),
    }
}

/// Reads the client's JSON status. `Some(true)` is a subscription login,
/// `Some(false)` is another signed-in credential, and `None` is no login.
fn claude_status(stdout: &[u8]) -> Option<bool> {
    let status: Value = serde_json::from_slice(stdout).ok()?;
    let logged_in = status
        .get("loggedIn")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !logged_in {
        return None;
    }
    let method = status
        .get("authMethod")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(matches!(method, "claude.ai" | "oauth_token"))
}

fn connect(state: &AppState, attempt: &Arc<Mutex<Attempt>>) -> Result<Provider, SignInReason> {
    let (vendor, name, provider_id) = {
        let attempt = lock(attempt);
        (
            attempt.vendor.clone(),
            attempt.name.clone(),
            attempt.provider_id.clone(),
        )
    };
    let name = name.unwrap_or_else(|| {
        crate::providers::find(&vendor)
            .map(|definition| definition.label.to_owned())
            .unwrap_or_else(|| vendor.clone())
    });

    let provider = if let Some(id) = provider_id {
        let mut provider = state.provider(&id).ok_or(SignInReason::Internal)?;
        provider.name = name;
        provider.auth = Auth::Subscription;
        provider.connected = true;
        provider.sample = false;
        provider.plan = "subscription".to_owned();
        provider.rows.clear();
        provider.foot = String::new();
        provider.usage_read_at = None;
        provider.usage_checked_at = None;
        provider.usage_status = None;
        provider.usage_source = None;
        state
            .replace_provider(provider)
            .ok_or(SignInReason::Internal)?
    } else {
        state.add_provider(Provider {
            id: String::new(),
            name,
            vendor,
            auth: Auth::Subscription,
            server: None,
            api_style: None,
            sample: false,
            connected: true,
            plan: "subscription".to_owned(),
            rows: Vec::new(),
            minimum_balance: None,
            foot: String::new(),
            usage_read_at: None,
            usage_checked_at: None,
            usage_status: None,
            usage_source: None,
            limit_reported_at: None,
        })
    };

    state.save();
    state.publish_provider(provider.clone());
    Ok(provider)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_found_and_trimmed() {
        assert_eq!(
            find_url("Open https://example.com/login?code=1, then continue."),
            Some("https://example.com/login?code=1".to_owned())
        );
        assert_eq!(
            find_url("Visit http://127.0.0.1:1455/auth to sign in"),
            Some("http://127.0.0.1:1455/auth".to_owned())
        );
        assert_eq!(find_url("No address here"), None);
    }

    #[test]
    fn device_codes_are_found_only_in_code_lines() {
        assert_eq!(
            find_device_code("Enter this code: ABCD-EFGH"),
            Some("ABCD-EFGH".to_owned())
        );
        assert_eq!(
            find_device_code("2. Code: AAAAB-BB6TU"),
            Some("AAAAB-BB6TU".to_owned())
        );
        assert_eq!(
            find_device_code("Open https://example.com/code/ABCD-EFGH"),
            None
        );
        assert_eq!(find_device_code("The code is short"), None);
        assert_eq!(find_device_code("ABCD-EFGH"), None);
    }

    #[test]
    fn terminal_escapes_do_not_hide_a_url() {
        let line = "\u{1b}[1mOpen\u{1b}[0m \u{1b}[34mhttps://example.com/signin\u{1b}[0m";
        assert_eq!(
            find_url(&strip_terminal_codes(line)),
            Some("https://example.com/signin".to_owned())
        );
    }

    #[test]
    fn success_hints_cover_the_client_wording() {
        assert!(is_success_hint("Login successful"));
        assert!(is_success_hint("Logged in using ChatGPT"));
        assert!(is_success_hint("Successfully logged in."));
        assert!(!is_success_hint("Waiting for the browser"));
    }

    #[test]
    fn claude_status_distinguishes_subscriptions_from_keys() {
        assert_eq!(
            claude_status(br#"{"loggedIn":true,"authMethod":"claude.ai"}"#),
            Some(true)
        );
        assert_eq!(
            claude_status(br#"{"loggedIn":true,"authMethod":"oauth_token"}"#),
            Some(true)
        );
        assert_eq!(
            claude_status(br#"{"loggedIn":true,"authMethod":"api_key"}"#),
            Some(false)
        );
        assert_eq!(
            claude_status(br#"{"loggedIn":false,"authMethod":"none"}"#),
            None
        );
        assert_eq!(claude_status(b"not json"), None);
    }

    #[test]
    fn login_commands_follow_the_client_and_the_mode() {
        assert_eq!(
            login_arguments(Client::ClaudeCode, SignInMode::Browser),
            vec!["auth", "login"]
        );
        assert_eq!(
            login_arguments(Client::Codex, SignInMode::Browser),
            vec!["login"]
        );
        assert_eq!(
            login_arguments(Client::Codex, SignInMode::Code),
            vec!["login", "--device-auth"]
        );
        assert_eq!(status_arguments(Client::ClaudeCode), vec!["auth", "status"]);
        assert_eq!(status_arguments(Client::Codex), vec!["login", "status"]);
    }

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(validated_name(Some("  ")).expect("blank"), None);
        assert_eq!(
            validated_name(Some(" My Claude ")).expect("name"),
            Some("My Claude".to_owned())
        );
        assert!(validated_name(Some(&"x".repeat(81))).is_err());
        assert!(validated_name(Some("bad\u{7}name")).is_err());
    }
}
