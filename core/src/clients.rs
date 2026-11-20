use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::private_fs;
use crate::state::{AgentCheck, Client};

const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const INSTALL_VERIFY_TIMEOUT: Duration = Duration::from_secs(10);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const VERSION_POLL_INTERVAL: Duration = Duration::from_millis(25);
const INSTALL_POLL_INTERVAL: Duration = Duration::from_millis(50);
const INSTALL_LOG_LIMIT: usize = 4_000;
const MAX_JOBS: usize = 32;

const CURL: &str = "/usr/bin/curl";
const SHELL: &str = "/bin/bash";
const SAFE_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

const CLAUDE_VERSION_URL: &str = "https://downloads.claude.ai/claude-code-releases/latest";
const CODEX_VERSION_URL: &str = "https://releases.openai.com/codex/channels/latest";
const OPENCODE_VERSION_URL: &str = "https://registry.npmjs.org/opencode-ai/latest";
const ANTIGRAVITY_MANIFEST_BASE: &str =
    "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests";

#[derive(Clone, Debug, Default)]
pub(crate) struct DiscoveryConfig {
    pub(crate) path: Option<OsString>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) bundle_dir: Option<PathBuf>,
    pub(crate) system_dirs: Vec<PathBuf>,
}

impl DiscoveryConfig {
    pub(crate) fn from_process(data_dir: Option<PathBuf>) -> Self {
        Self {
            path: env::var_os("PATH"),
            home: crate::seed::home_dir(),
            data_dir,
            bundle_dir: env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf)),
            system_dirs: vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
            ],
        }
    }
}

/// Where the found executable comes from. Only the app's own copy can be
/// replaced from inside the app.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientCopy {
    App,
    User,
    Bundled,
}

#[derive(Clone, Debug)]
pub(crate) struct FoundClient {
    pub(crate) path: PathBuf,
    pub(crate) copy: ClientCopy,
}

pub fn find_client(client: Client) -> Option<PathBuf> {
    let config = DiscoveryConfig::from_process(crate::persist::data_directory_from_environment());
    find_client_in(client, &config)
}

pub(crate) fn find_client_in(client: Client, config: &DiscoveryConfig) -> Option<PathBuf> {
    locate_client_in(client, config).map(|found| found.path)
}

pub(crate) fn locate_client_in(client: Client, config: &DiscoveryConfig) -> Option<FoundClient> {
    let path = find_in_directories(candidate_directories(client, config), client.binary())?;
    let copy = classify_copy(&path, client, config);
    Some(FoundClient { path, copy })
}

/// Names the source of a found executable by the known folder that holds it.
fn classify_copy(path: &Path, client: Client, config: &DiscoveryConfig) -> ClientCopy {
    if config
        .data_dir
        .as_deref()
        .is_some_and(|data_dir| path_is_within(path, &install_root(data_dir, client)))
    {
        return ClientCopy::App;
    }
    if config.bundle_dir.as_deref().is_some_and(|bundle_dir| {
        bundle_directories(bundle_dir, client)
            .iter()
            .any(|directory| path_is_within(path, directory))
    }) {
        return ClientCopy::Bundled;
    }
    ClientCopy::User
}

fn path_is_within(path: &Path, directory: &Path) -> bool {
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    path.starts_with(directory)
}

fn candidate_directories(client: Client, config: &DiscoveryConfig) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(path_value) = &config.path {
        for directory in env::split_paths(path_value) {
            push_directory(&mut directories, directory);
        }
    }
    if let Some(home) = &config.home {
        for directory in [
            home.join(".local").join("bin"),
            home.join(".npm-global").join("bin"),
            home.join(".npm").join("bin"),
            home.join(".volta").join("bin"),
        ] {
            push_directory(&mut directories, directory);
        }
        for directory in node_directories(home) {
            push_directory(&mut directories, directory);
        }
        if client == Client::Antigravity {
            for directory in antigravity_directories(home) {
                push_directory(&mut directories, directory);
            }
        }
    }
    for directory in &config.system_dirs {
        push_directory(&mut directories, directory.clone());
    }
    if let Some(bundle_dir) = &config.bundle_dir {
        for directory in bundle_directories(bundle_dir, client) {
            push_directory(&mut directories, directory);
        }
    }
    if let Some(data_dir) = &config.data_dir {
        for directory in installed_directories(&install_root(data_dir, client), client) {
            push_directory(&mut directories, directory);
        }
    }
    directories
}

fn push_directory(directories: &mut Vec<PathBuf>, directory: PathBuf) {
    if directory.is_absolute() && !directories.contains(&directory) {
        directories.push(directory);
    }
}

fn antigravity_directories(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".antigravity").join("bin"),
        home.join(".gemini").join("antigravity-cli").join("bin"),
        home.join(".gemini").join("antigravity-cli"),
    ]
}

fn node_directories(home: &Path) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for (root, tail) in [
        (home.join(".nvm").join("versions").join("node"), "bin"),
        (
            home.join(".local")
                .join("share")
                .join("fnm")
                .join("node-versions"),
            "installation/bin",
        ),
        (home.join(".fnm").join("node-versions"), "installation/bin"),
    ] {
        for version in versioned_children(&root) {
            directories.push(version.join(tail));
        }
    }
    directories
}

fn versioned_children(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut versions = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| {
        version_key(right)
            .cmp(&version_key(left))
            .then_with(|| right.cmp(left))
    });
    versions
}

fn version_key(path: &Path) -> Vec<u64> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim_start_matches('v');
    name.split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn find_in_directories(
    directories: impl IntoIterator<Item = PathBuf>,
    binary: &str,
) -> Option<PathBuf> {
    directories
        .into_iter()
        .map(|directory| directory.join(binary))
        .find_map(canonical_executable)
}

fn canonical_executable(path: PathBuf) -> Option<PathBuf> {
    if !is_executable(&path) {
        return None;
    }
    path.canonicalize().ok()
}

pub fn check_all(data_dir: Option<&Path>) -> Vec<AgentCheck> {
    let config = DiscoveryConfig::from_process(data_dir.map(Path::to_path_buf));
    check_all_in(&config)
}

fn check_all_in(config: &DiscoveryConfig) -> Vec<AgentCheck> {
    thread::scope(|scope| {
        let handles = Client::ALL.map(|client| scope.spawn(move || check(client, config)));
        Client::ALL
            .into_iter()
            .zip(handles)
            .map(|(client, handle)| handle.join().unwrap_or_else(|_| missing_check(client)))
            .collect()
    })
}

pub fn missing_checks() -> Vec<AgentCheck> {
    Client::ALL.into_iter().map(missing_check).collect()
}

fn missing_check(client: Client) -> AgentCheck {
    AgentCheck {
        name: client,
        installed: false,
        version: None,
        path: None,
        copy: None,
    }
}

fn check(client: Client, config: &DiscoveryConfig) -> AgentCheck {
    let Some(found) = locate_client_in(client, config) else {
        return missing_check(client);
    };

    let version = probe_version(&found.path, VERSION_TIMEOUT).unwrap_or_default();
    AgentCheck {
        name: client,
        installed: true,
        version,
        path: Some(found.path.to_string_lossy().into_owned()),
        copy: Some(found.copy),
    }
}

fn probe_version(path: &Path, timeout: Duration) -> Result<Option<String>, String> {
    let mut child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not run {}: {error}", path.display()))?;

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return Ok(child
                    .wait_with_output()
                    .ok()
                    .and_then(|output| version_from_output(&output.stdout, &output.stderr)));
            }
            Ok(Some(status)) => {
                return Err(format!(
                    "{} --version exited with {status}.",
                    path.display()
                ));
            }
            Ok(None) => thread::sleep(VERSION_POLL_INTERVAL),
            Err(error) => {
                return Err(format!(
                    "Could not read the version of {}: {error}",
                    path.display()
                ));
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    Err(format!("{} --version timed out.", path.display()))
}

fn version_from_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    for bytes in [stdout, stderr] {
        let text = String::from_utf8_lossy(bytes);
        if let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) {
            return Some(line.chars().take(200).collect());
        }
    }
    None
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientUpdateCheck {
    pub client: Client,
    pub copy: Option<ClientCopy>,
    pub installed: bool,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub can_update: bool,
    pub error: Option<String>,
}

/// Reads the vendor's current version and compares it with the found copy.
/// It reads only version metadata and never downloads or runs an installer.
pub fn check_update(client: Client, data_dir: Option<&Path>) -> ClientUpdateCheck {
    let config = DiscoveryConfig::from_process(data_dir.map(Path::to_path_buf));
    check_update_in(client, &config)
}

fn check_update_in(client: Client, config: &DiscoveryConfig) -> ClientUpdateCheck {
    let Some(found) = locate_client_in(client, config) else {
        return ClientUpdateCheck {
            client,
            copy: None,
            installed: false,
            current_version: None,
            latest_version: None,
            update_available: false,
            can_update: false,
            error: None,
        };
    };

    let current_version = probe_version(&found.path, VERSION_TIMEOUT).ok().flatten();
    let (latest_version, error) = match vendor_latest_version(client) {
        Ok(version) => (Some(version), None),
        Err(error) => (None, Some(error)),
    };
    let update_available = latest_version
        .as_deref()
        .zip(current_version.as_deref())
        .is_some_and(|(latest, current)| is_newer_version(latest, current));

    ClientUpdateCheck {
        client,
        copy: Some(found.copy),
        installed: true,
        current_version,
        latest_version,
        update_available,
        can_update: found.copy == ClientCopy::App,
        error,
    }
}

fn vendor_latest_version(client: Client) -> Result<String, String> {
    let url = latest_version_url(client)?;
    let output = Command::new(CURL)
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
        ])
        .arg(VERSION_CHECK_TIMEOUT.as_secs().to_string())
        .arg(&url)
        .env("PATH", SAFE_PATH)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Could not check the vendor version: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "The vendor version check failed with {}.",
            output.status
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_latest_version(client, &text)
        .ok_or_else(|| "The vendor did not return a version.".to_owned())
}

fn latest_version_url(client: Client) -> Result<String, String> {
    Ok(match client {
        Client::ClaudeCode => CLAUDE_VERSION_URL.to_owned(),
        Client::Codex => CODEX_VERSION_URL.to_owned(),
        Client::OpenCode2 => OPENCODE_VERSION_URL.to_owned(),
        Client::Antigravity => {
            format!(
                "{ANTIGRAVITY_MANIFEST_BASE}/{}.json",
                antigravity_platform()?
            )
        }
    })
}

/// Builds the platform name that the Antigravity manifest path uses.
fn antigravity_platform() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => return Err(format!("Antigravity has no version check for {other}.")),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => return Err(format!("Antigravity has no version check for {other}.")),
    };
    Ok(format!("{os}_{arch}"))
}

fn parse_latest_version(client: Client, body: &str) -> Option<String> {
    let value = match client {
        Client::ClaudeCode => body.lines().map(str::trim).find(|line| !line.is_empty())?,
        Client::Codex => json_string_field(body, "tag_name")?,
        Client::OpenCode2 | Client::Antigravity => json_string_field(body, "version")?,
    };
    let value = value.trim().trim_start_matches('v');
    let value = value.strip_prefix("rust-").unwrap_or(value);
    let value = value.trim_start_matches('v');
    version_numbers(value)?;
    Some(value.chars().take(64).collect())
}

fn json_string_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let after_key = &text[text.find(&needle)? + needle.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    let after_quote = after_colon.strip_prefix('"')?;
    let end = after_quote.find('"')?;
    Some(&after_quote[..end])
}

/// Returns the numbers of the first dotted version in the text.
fn version_numbers(text: &str) -> Option<Vec<u64>> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        let token = text[start..index].trim_end_matches('.');
        if token.contains('.') {
            let parts = token
                .split('.')
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>();
            if let Ok(parts) = parts {
                return Some(parts);
            }
        }
    }
    None
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let (Some(latest), Some(current)) = (version_numbers(latest), version_numbers(current)) else {
        return false;
    };
    let length = latest.len().max(current.len());
    for index in 0..length {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

pub(crate) fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallState {
    Running,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallStage {
    Preparing,
    Downloading,
    Installing,
    Verifying,
    Finishing,
}

impl InstallStage {
    fn step(self) -> u8 {
        match self {
            InstallStage::Preparing => 1,
            InstallStage::Downloading => 2,
            InstallStage::Installing => 3,
            InstallStage::Verifying => 4,
            InstallStage::Finishing => 5,
        }
    }

    fn total() -> u8 {
        5
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInstall {
    pub id: String,
    pub client: Client,
    pub state: InstallState,
    pub stage: InstallStage,
    pub step: u8,
    pub total: u8,
    pub version: Option<String>,
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallStartError {
    Busy,
    NoDataDirectory,
}

impl std::fmt::Display for InstallStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallStartError::Busy => {
                formatter.write_str("An install is already running for this client.")
            }
            InstallStartError::NoDataDirectory => {
                formatter.write_str("Client installation needs a data directory.")
            }
        }
    }
}

pub(crate) trait InstallBackend: Send + Sync {
    fn install(
        &self,
        client: Client,
        staging: &Path,
        progress: &dyn Fn(InstallStage),
    ) -> Result<(), String>;
}

struct VendorInstaller;

impl InstallBackend for VendorInstaller {
    fn install(
        &self,
        client: Client,
        staging: &Path,
        progress: &dyn Fn(InstallStage),
    ) -> Result<(), String> {
        progress(InstallStage::Downloading);
        private_fs::create_private_dir_all(&staging.join("home"))
            .map_err(|error| format!("Could not prepare the install folder: {error}"))?;
        let script = staging.join("install.sh");
        download(client.install_url(), &script, staging)?;

        progress(InstallStage::Installing);
        let mut command = Command::new(SHELL);
        command.arg(&script);
        match client {
            Client::OpenCode2 => {
                command.arg("--no-modify-path");
            }
            Client::Antigravity => {
                command.arg("--dir");
                command.arg(staging.join("bin"));
                command.args(["--skip-path", "--skip-aliases"]);
            }
            Client::ClaudeCode | Client::Codex => {}
        }
        command
            .env("HOME", staging.join("home"))
            .env("PATH", SAFE_PATH)
            .stdin(Stdio::null());
        match client {
            Client::Codex => {
                command
                    .env("CODEX_HOME", staging.join("home").join(".codex"))
                    .env("CODEX_INSTALL_DIR", staging.join("bin"))
                    .env("CODEX_NON_INTERACTIVE", "1");
            }
            Client::ClaudeCode | Client::OpenCode2 | Client::Antigravity => {}
        }
        run_to_log(command, &staging.join("install.log"), INSTALL_TIMEOUT)
    }
}

struct CommandInstaller {
    program: PathBuf,
}

impl InstallBackend for CommandInstaller {
    fn install(
        &self,
        client: Client,
        staging: &Path,
        progress: &dyn Fn(InstallStage),
    ) -> Result<(), String> {
        progress(InstallStage::Downloading);
        progress(InstallStage::Installing);
        private_fs::create_private_dir_all(&staging.join("home"))
            .map_err(|error| format!("Could not prepare the install folder: {error}"))?;
        let mut command = Command::new(&self.program);
        command
            .arg(client.slug())
            .arg(staging)
            .arg(client.binary())
            .env("HOME", staging.join("home"))
            .stdin(Stdio::null());
        run_to_log(command, &staging.join("install.log"), INSTALL_TIMEOUT)
    }
}

fn download(url: &str, destination: &Path, staging: &Path) -> Result<(), String> {
    if !Path::new(CURL).is_file() {
        return Err("The system download tool was not found.".to_owned());
    }
    let mut command = Command::new(CURL);
    command
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--retry",
            "2",
        ])
        .arg("--output")
        .arg(destination)
        .arg(url)
        .env("PATH", SAFE_PATH)
        .stdin(Stdio::null());
    run_to_log(command, &staging.join("install.log"), INSTALL_TIMEOUT)
}

fn run_to_log(mut command: Command, log: &Path, timeout: Duration) -> Result<(), String> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|error| format!("Could not write the install log: {error}"))?;
    let error_log = file
        .try_clone()
        .map_err(|error| format!("Could not write the install log: {error}"))?;
    command
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(error_log));

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start the installer: {error}"))?;
    if let Err(error) = wait_for(&mut child, timeout) {
        let detail = log_tail(log);
        return Err(if detail.is_empty() {
            error
        } else {
            format!("{error} {detail}")
        });
    }
    Ok(())
}

fn log_tail(log: &Path) -> String {
    let Ok(bytes) = fs::read(log) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let start = text.len().saturating_sub(INSTALL_LOG_LIMIT);
    let start = text
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= start)
        .unwrap_or(text.len());
    crate::text::clean_label(&text[start..], INSTALL_LOG_LIMIT * 2).unwrap_or_default()
}

fn wait_for(child: &mut Child, timeout: Duration) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(status),
            Ok(Some(status)) => {
                return Err(format!("The installer failed with {status}."));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(INSTALL_POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The install timed out.".to_owned());
            }
            Err(error) => return Err(format!("Could not wait for the installer: {error}")),
        }
    }
}

struct InstallRegistry {
    jobs: HashMap<String, ClientInstall>,
    order: Vec<String>,
    active: HashMap<Client, String>,
}

static DISCOVERY_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Changes when an install changes the app's own copy, so a cached health
/// check is not reused.
pub(crate) fn discovery_generation() -> u64 {
    DISCOVERY_GENERATION.load(Ordering::Relaxed)
}

static INSTALLS: LazyLock<Mutex<InstallRegistry>> = LazyLock::new(|| {
    Mutex::new(InstallRegistry {
        jobs: HashMap::new(),
        order: Vec::new(),
        active: HashMap::new(),
    })
});

fn registry() -> MutexGuard<'static, InstallRegistry> {
    INSTALLS.lock().unwrap_or_else(|error| error.into_inner())
}

pub fn start_install(
    client: Client,
    data_dir: impl Into<PathBuf>,
) -> Result<ClientInstall, InstallStartError> {
    let data_dir = data_dir.into();
    if !data_dir.is_absolute() {
        return Err(InstallStartError::NoDataDirectory);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let job = ClientInstall {
        id: id.clone(),
        client,
        state: InstallState::Running,
        stage: InstallStage::Preparing,
        step: InstallStage::Preparing.step(),
        total: InstallStage::total(),
        version: None,
        path: None,
        error: None,
    };

    {
        let mut registry = registry();
        if registry.active.contains_key(&client) {
            return Err(InstallStartError::Busy);
        }
        registry.active.insert(client, id.clone());
        registry.jobs.insert(id.clone(), job.clone());
        registry.order.push(id.clone());
        prune_jobs(&mut registry);
    }

    let backend = install_backend();
    thread::spawn(move || run_install(id, client, data_dir, backend));
    Ok(job)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateStartError {
    Busy,
    NoDataDirectory,
    NotInstalled,
    NotAppOwned,
}

impl std::fmt::Display for UpdateStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateStartError::Busy => formatter.write_str("An update is already running."),
            UpdateStartError::NoDataDirectory => {
                formatter.write_str("Client update needs a data directory.")
            }
            UpdateStartError::NotInstalled => formatter.write_str("The client is not installed."),
            UpdateStartError::NotAppOwned => formatter.write_str(
                "This copy is managed outside elesdee. Update it with the tool that installed it.",
            ),
        }
    }
}

/// Replaces the app's own copy with the vendor's current build. A copy from
/// the user or the app bundle is never touched.
pub fn start_update(
    client: Client,
    data_dir: impl Into<PathBuf>,
) -> Result<ClientInstall, UpdateStartError> {
    let data_dir = data_dir.into();
    if !data_dir.is_absolute() {
        return Err(UpdateStartError::NoDataDirectory);
    }

    let config = DiscoveryConfig::from_process(Some(data_dir.clone()));
    update_target(client, &config)?;
    start_install(client, data_dir).map_err(|error| match error {
        InstallStartError::Busy => UpdateStartError::Busy,
        InstallStartError::NoDataDirectory => UpdateStartError::NoDataDirectory,
    })
}

fn update_target(client: Client, config: &DiscoveryConfig) -> Result<(), UpdateStartError> {
    match locate_client_in(client, config) {
        None => Err(UpdateStartError::NotInstalled),
        Some(found) if found.copy != ClientCopy::App => Err(UpdateStartError::NotAppOwned),
        Some(_) => Ok(()),
    }
}

pub fn install_status(id: &str) -> Option<ClientInstall> {
    registry().jobs.get(id).cloned()
}

fn prune_jobs(registry: &mut InstallRegistry) {
    if registry.jobs.len() <= MAX_JOBS {
        return;
    }
    let active = registry.active.values().cloned().collect::<Vec<_>>();
    let mut index = 0;
    while registry.jobs.len() > MAX_JOBS && index < registry.order.len() {
        let id = registry.order[index].clone();
        if !active.contains(&id) && registry.jobs.remove(&id).is_some() {
            registry.order.remove(index);
        } else {
            index += 1;
        }
    }
}

fn install_backend() -> Arc<dyn InstallBackend> {
    let override_bin = cfg!(debug_assertions)
        .then(|| env::var_os("ELESDEE_INSTALL_BIN"))
        .flatten()
        .filter(|value| !value.is_empty());
    match override_bin {
        Some(program) => Arc::new(CommandInstaller {
            program: PathBuf::from(program),
        }),
        None => Arc::new(VendorInstaller),
    }
}

fn run_install(id: String, client: Client, data_dir: PathBuf, backend: Arc<dyn InstallBackend>) {
    let clients_dir = data_dir.join("clients");
    let root = install_root(&data_dir, client);
    let staging = clients_dir.join(format!(".install-{}-{}", client.slug(), id));
    let outcome = install_into(&id, client, &root, &staging, backend.as_ref());
    let _ = fs::remove_dir_all(&staging);

    let mut registry = registry();
    if let Some(job) = registry.jobs.get_mut(&id) {
        match outcome {
            Ok((relative, version)) => {
                DISCOVERY_GENERATION.fetch_add(1, Ordering::Relaxed);
                job.state = InstallState::Done;
                job.stage = InstallStage::Finishing;
                job.step = InstallStage::Finishing.step();
                job.version = Some(version);
                job.path = Some(root.join(relative).to_string_lossy().into_owned());
            }
            Err(error) => {
                job.state = InstallState::Failed;
                job.error = Some(error);
            }
        }
    }
    registry.active.remove(&client);
}

fn install_into(
    id: &str,
    client: Client,
    root: &Path,
    staging: &Path,
    backend: &dyn InstallBackend,
) -> Result<(PathBuf, String), String> {
    set_stage(id, InstallStage::Preparing);
    private_fs::create_private_dir_all(staging)
        .map_err(|error| format!("Could not prepare the install folder: {error}"))?;
    backend.install(client, staging, &|stage| set_stage(id, stage))?;

    set_stage(id, InstallStage::Verifying);
    let binary = installed_binary(client, staging).ok_or_else(|| {
        format!(
            "The installer finished but {} was not found.",
            client.display_name()
        )
    })?;
    let version = probe_version(&binary, INSTALL_VERIFY_TIMEOUT)?
        .ok_or_else(|| "The installed client did not report a version.".to_owned())?;
    let relative = binary
        .strip_prefix(staging)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| PathBuf::from(client.binary()));

    set_stage(id, InstallStage::Finishing);
    let _ = fs::remove_file(staging.join("install.sh"));
    let _ = fs::remove_file(staging.join("install.log"));
    rebase_absolute_symlinks(staging, root)?;
    replace_install(staging, root, id, client)?;
    Ok((relative, version))
}

/// Rewrites absolute links that point into the staging folder so they point
/// into the final folder. Vendor installers create links with absolute
/// targets, and the staging folder is renamed when the install finishes.
fn rebase_absolute_symlinks(staging: &Path, root: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let canonical_staging = staging.canonicalize().ok();
        let mut pending = vec![staging.to_path_buf()];
        while let Some(directory) = pending.pop() {
            let entries = fs::read_dir(&directory)
                .map_err(|error| format!("Could not read the install folder: {error}"))?;
            for entry in entries {
                let entry =
                    entry.map_err(|error| format!("Could not read the install folder: {error}"))?;
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .map_err(|error| format!("Could not read the install folder: {error}"))?;
                if file_type.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !file_type.is_symlink() {
                    continue;
                }
                let target = fs::read_link(&path)
                    .map_err(|error| format!("Could not read a link in the install: {error}"))?;
                let relative = target.strip_prefix(staging).ok().or_else(|| {
                    canonical_staging
                        .as_deref()
                        .and_then(|base| target.strip_prefix(base).ok())
                });
                let Some(relative) = relative else {
                    continue;
                };
                let next = root.join(relative);
                fs::remove_file(&path)
                    .map_err(|error| format!("Could not update a link in the install: {error}"))?;
                symlink(&next, &path)
                    .map_err(|error| format!("Could not update a link in the install: {error}"))?;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (staging, root);
    }
    Ok(())
}

fn set_stage(id: &str, stage: InstallStage) {
    let mut registry = registry();
    if let Some(job) = registry.jobs.get_mut(id)
        && job.state == InstallState::Running
    {
        job.stage = stage;
        job.step = stage.step();
    }
}

fn replace_install(staging: &Path, root: &Path, id: &str, client: Client) -> Result<(), String> {
    let clients_dir = root
        .parent()
        .ok_or_else(|| "The install folder is not valid.".to_owned())?;
    let backup = clients_dir.join(format!(".previous-{}-{}", client.slug(), id));
    let had_existing = root.exists();
    if had_existing {
        fs::rename(root, &backup)
            .map_err(|error| format!("Could not replace the current copy: {error}"))?;
    }
    if let Err(error) = fs::rename(staging, root) {
        if had_existing {
            let _ = fs::rename(&backup, root);
        }
        return Err(format!("Could not finish the install: {error}"));
    }
    if had_existing {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

fn install_root(data_dir: &Path, client: Client) -> PathBuf {
    data_dir.join("clients").join(client.slug())
}

fn bundle_directories(bundle_dir: &Path, client: Client) -> Vec<PathBuf> {
    vec![
        bundle_dir.join("clients").join(client.slug()).join("bin"),
        bundle_dir
            .join("..")
            .join("Resources")
            .join("clients")
            .join(client.slug())
            .join("bin"),
    ]
}

fn installed_directories(root: &Path, client: Client) -> Vec<PathBuf> {
    let mut directories = vec![root.join("bin")];
    match client {
        Client::ClaudeCode => directories.push(root.join("home").join(".local").join("bin")),
        Client::OpenCode2 => directories.push(root.join("home").join(".opencode").join("bin")),
        Client::Codex | Client::Antigravity => {}
    }
    directories
}

fn installed_binary(client: Client, root: &Path) -> Option<PathBuf> {
    installed_directories(root, client)
        .into_iter()
        .map(|directory| directory.join(client.binary()))
        .find(|path| is_executable(path))
}

impl Client {
    pub fn slug(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude-code",
            Client::Codex => "codex",
            Client::OpenCode2 => "opencode-2",
            Client::Antigravity => "antigravity",
        }
    }

    pub fn from_slug(value: &str) -> Option<Client> {
        Client::ALL
            .into_iter()
            .find(|client| client.slug().eq_ignore_ascii_case(value))
    }

    fn install_url(self) -> &'static str {
        match self {
            Client::ClaudeCode => "https://claude.ai/install.sh",
            Client::Codex => "https://chatgpt.com/codex/install.sh",
            Client::OpenCode2 => "https://opencode.ai/install",
            Client::Antigravity => "https://antigravity.google/cli/install.sh",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct TempDirectory {
        path: PathBuf,
    }

    impl TempDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "elesdee-clients-{label}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self { path }
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn executable(directory: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(directory).expect("create directory");
        let path = directory.join(name);
        fs::write(&path, "#!/bin/sh\nprintf '1.2.3\\n'\n").expect("write test executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make executable");
        path.canonicalize().expect("canonical test executable")
    }

    fn config(
        path: Option<&Path>,
        home: Option<&Path>,
        data_dir: Option<&Path>,
        system_dirs: Vec<&Path>,
    ) -> DiscoveryConfig {
        DiscoveryConfig {
            path: path.map(|value| value.as_os_str().to_owned()),
            home: home.map(Path::to_path_buf),
            data_dir: data_dir.map(Path::to_path_buf),
            bundle_dir: None,
            system_dirs: system_dirs.into_iter().map(Path::to_path_buf).collect(),
        }
    }

    #[test]
    fn clients_have_expected_binaries() {
        assert_eq!(Client::ClaudeCode.binary(), "claude");
        assert_eq!(Client::Codex.binary(), "codex");
        assert_eq!(Client::OpenCode2.binary(), "opencode");
        assert_eq!(Client::Antigravity.binary(), "agy");
        assert!(Client::ALL.contains(&Client::Antigravity));
        assert_eq!(Client::from_slug("claude-code"), Some(Client::ClaudeCode));
        assert_eq!(Client::from_slug("opencode-2"), Some(Client::OpenCode2));
        assert_eq!(Client::from_slug("nope"), None);
    }

    #[test]
    fn path_wins_over_the_known_install_folders() {
        let temp = TempDirectory::new("order");
        let path_dir = temp.path.join("path");
        let home = temp.path.join("home");
        let expected = executable(&path_dir, "claude");
        executable(&home.join(".local/bin"), "claude");
        executable(&temp.path.join("homebrew"), "claude");

        let found = find_client_in(
            Client::ClaudeCode,
            &config(
                Some(&path_dir),
                Some(&home),
                None,
                vec![&temp.path.join("homebrew")],
            ),
        );
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn user_folders_win_over_homebrew() {
        let temp = TempDirectory::new("user");
        let home = temp.path.join("home");
        let expected = executable(&home.join(".local/bin"), "codex");
        executable(&temp.path.join("homebrew"), "codex");

        let found = find_client_in(
            Client::Codex,
            &config(None, Some(&home), None, vec![&temp.path.join("homebrew")]),
        );
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn npm_and_node_version_folders_are_searched() {
        let temp = TempDirectory::new("npm");
        let home = temp.path.join("home");
        let npm_global = executable(&home.join(".npm-global/bin"), "opencode");
        executable(&home.join(".nvm/versions/node/v9.1.0/bin"), "opencode");
        executable(&home.join(".nvm/versions/node/v24.2.0/bin"), "opencode");

        let found = find_client_in(
            Client::OpenCode2,
            &config(None, Some(&home), None, Vec::new()),
        );
        assert_eq!(found, Some(npm_global));
    }

    #[test]
    fn the_newest_node_version_is_searched_first() {
        let temp = TempDirectory::new("node-order");
        let home = temp.path.join("home");
        executable(&home.join(".nvm/versions/node/v9.1.0/bin"), "codex");
        let newest = executable(&home.join(".nvm/versions/node/v24.2.0/bin"), "codex");

        let found = find_client_in(Client::Codex, &config(None, Some(&home), None, Vec::new()));
        assert_eq!(found, Some(newest));
    }

    #[test]
    fn homebrew_folders_are_used_when_nothing_else_has_the_client() {
        let temp = TempDirectory::new("homebrew");
        let arm = temp.path.join("arm");
        let intel = temp.path.join("intel");
        let expected = executable(&arm, "agy");
        executable(&intel, "agy");

        let found = find_client_in(
            Client::Antigravity,
            &config(None, None, None, vec![&arm, &intel]),
        );
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn antigravity_has_its_vendor_folders() {
        let temp = TempDirectory::new("antigravity");
        let home = temp.path.join("home");
        let expected = executable(&home.join(".gemini/antigravity-cli/bin"), "agy");

        let found = find_client_in(
            Client::Antigravity,
            &config(None, Some(&home), None, Vec::new()),
        );
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn the_apps_own_copy_is_the_last_choice() {
        let temp = TempDirectory::new("own-copy");
        let data_dir = temp.path.join("data");
        let expected = executable(
            &install_root(&data_dir, Client::ClaudeCode).join("home/.local/bin"),
            "claude",
        );

        let found = find_client_in(
            Client::ClaudeCode,
            &config(None, None, Some(&data_dir), Vec::new()),
        );
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn a_copy_inside_the_app_bundle_is_found() {
        let temp = TempDirectory::new("bundled");
        let bundle_dir = temp.path.join("MacOS");
        let expected = executable(&bundle_dir.join("clients/claude-code/bin"), "claude");
        let mut discovery = config(None, None, None, Vec::new());
        discovery.bundle_dir = Some(bundle_dir);

        assert_eq!(
            find_client_in(Client::ClaudeCode, &discovery),
            Some(expected)
        );
    }

    #[test]
    fn non_executable_files_are_ignored() {
        let temp = TempDirectory::new("not-executable");
        let home = temp.path.join("home");
        let directory = home.join(".local/bin");
        fs::create_dir_all(&directory).expect("create directory");
        let path = directory.join("claude");
        fs::write(&path, "not a program").expect("write file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("set permissions");

        assert_eq!(
            find_client_in(
                Client::ClaudeCode,
                &config(None, Some(&home), None, Vec::new())
            ),
            None
        );
    }

    #[test]
    fn relative_path_entries_are_ignored() {
        let temp = TempDirectory::new("relative-path");
        executable(&temp.path, "claude");

        let found = find_client_in(
            Client::ClaudeCode,
            &config(Some(Path::new(".")), Some(&temp.path), None, Vec::new()),
        );
        assert_eq!(found, None);
    }

    #[test]
    fn clients_report_the_path_and_version() {
        let temp = TempDirectory::new("check");
        let home = temp.path.join("home");
        executable(&home.join(".local/bin"), "codex");
        let checks = check_all_in(&config(None, Some(&home), None, Vec::new()));
        let codex = checks
            .iter()
            .find(|check| check.name == Client::Codex)
            .expect("codex check");
        assert!(codex.installed);
        assert_eq!(codex.version.as_deref(), Some("1.2.3"));
        assert!(codex.path.is_some());
        assert_eq!(codex.copy, Some(ClientCopy::User));
    }

    #[test]
    fn a_found_copy_is_classified_by_its_folder() {
        let temp = TempDirectory::new("copy-source");
        let home = temp.path.join("home");
        let data_dir = temp.path.join("data");
        let bundle_dir = temp.path.join("bundle");

        executable(&home.join(".local/bin"), "codex");
        assert_eq!(
            locate_client_in(
                Client::Codex,
                &config(None, Some(&home), Some(&data_dir), Vec::new())
            )
            .expect("user copy")
            .copy,
            ClientCopy::User
        );

        let app = executable(&install_root(&data_dir, Client::Codex).join("bin"), "codex");
        let mut discovery = config(None, None, Some(&data_dir), Vec::new());
        let found = locate_client_in(Client::Codex, &discovery).expect("app copy");
        assert_eq!(found.path, app);
        assert_eq!(found.copy, ClientCopy::App);

        let bundled = executable(&bundle_dir.join("clients/codex/bin"), "codex");
        discovery.bundle_dir = Some(bundle_dir);
        let found = locate_client_in(Client::Codex, &discovery).expect("bundled copy");
        assert_eq!(found.path, bundled);
        assert_eq!(found.copy, ClientCopy::Bundled);
    }

    #[test]
    fn only_the_apps_own_copy_can_be_updated() {
        let temp = TempDirectory::new("update-target");
        let home = temp.path.join("home");
        let data_dir = temp.path.join("data");

        assert_eq!(
            update_target(
                Client::Codex,
                &config(None, None, Some(&data_dir), Vec::new())
            ),
            Err(UpdateStartError::NotInstalled)
        );

        executable(&home.join(".local/bin"), "codex");
        assert_eq!(
            update_target(
                Client::Codex,
                &config(None, Some(&home), Some(&data_dir), Vec::new())
            ),
            Err(UpdateStartError::NotAppOwned)
        );

        executable(&install_root(&data_dir, Client::Codex).join("bin"), "codex");
        assert_eq!(
            update_target(
                Client::Codex,
                &config(None, None, Some(&data_dir), Vec::new())
            ),
            Ok(())
        );
    }

    #[test]
    fn vendor_version_responses_are_parsed() {
        assert_eq!(
            parse_latest_version(Client::ClaudeCode, "2.1.329\n").as_deref(),
            Some("2.1.329")
        );
        assert_eq!(
            parse_latest_version(Client::Codex, r#"{"tag_name":"rust-v0.171.0"}"#).as_deref(),
            Some("0.171.0")
        );
        assert_eq!(
            parse_latest_version(Client::OpenCode2, r#"{"version":"1.19.6"}"#).as_deref(),
            Some("1.19.6")
        );
        assert_eq!(
            parse_latest_version(
                Client::Antigravity,
                r#"{"version":"0.4.1","url":"https://example.test"}"#
            )
            .as_deref(),
            Some("0.4.1")
        );
        assert_eq!(
            parse_latest_version(Client::ClaudeCode, "<html>error</html>"),
            None
        );
        assert_eq!(parse_latest_version(Client::Codex, "{}"), None);
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer_version("2.0.0", "1.9.9 (Claude Code)"));
        assert!(is_newer_version("2.0.1", "2.0"));
        assert!(!is_newer_version("1.9.9", "2.0.0"));
        assert!(!is_newer_version("2.0", "2.0.0"));
        assert!(!is_newer_version("unknown", "1.0.0"));
    }

    struct FakeInstaller {
        outcome: Result<bool, String>,
    }

    impl InstallBackend for FakeInstaller {
        fn install(
            &self,
            client: Client,
            staging: &Path,
            progress: &dyn Fn(InstallStage),
        ) -> Result<(), String> {
            progress(InstallStage::Downloading);
            progress(InstallStage::Installing);
            match &self.outcome {
                Ok(executable_file) => {
                    let bin = staging.join("bin");
                    fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
                    let path = bin.join(client.binary());
                    if *executable_file {
                        fs::write(&path, "#!/bin/sh\nprintf '9.9.9\\n'\n")
                            .map_err(|error| error.to_string())?;
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                            .map_err(|error| error.to_string())?;
                    } else {
                        fs::write(&path, "not executable").map_err(|error| error.to_string())?;
                    }
                    Ok(())
                }
                Err(error) => Err(error.clone()),
            }
        }
    }

    fn run_install_for_test(
        client: Client,
        data_dir: &Path,
        backend: &dyn InstallBackend,
    ) -> (
        String,
        Option<String>,
        Option<String>,
        InstallState,
        PathBuf,
    ) {
        let id = uuid::Uuid::new_v4().to_string();
        let staging = data_dir
            .join("clients")
            .join(format!(".install-{}-{}", client.slug(), id));
        let root = install_root(data_dir, client);
        let result = install_into(&id, client, &root, &staging, backend);
        let _ = fs::remove_dir_all(&staging);
        match result {
            Ok((_, version)) => (
                id,
                Some(version),
                installed_binary(client, &root).map(|path| path.to_string_lossy().into_owned()),
                InstallState::Done,
                root,
            ),
            Err(error) => (id, None, Some(error), InstallState::Failed, root),
        }
    }

    #[test]
    fn install_verifies_the_version_and_moves_the_copy_into_place() {
        let temp = TempDirectory::new("install-ok");
        let (_, version, path, state, root) = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(true) },
        );
        assert_eq!(state, InstallState::Done);
        assert_eq!(version.as_deref(), Some("9.9.9"));
        assert!(path.is_some());
        assert!(root.join("bin/codex").is_file());
    }

    #[test]
    fn a_failed_install_leaves_no_partial_copy() {
        let temp = TempDirectory::new("install-fail");
        let (_, version, error, state, root) = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller {
                outcome: Err("network failed".to_owned()),
            },
        );
        assert_eq!(state, InstallState::Failed);
        assert_eq!(version, None);
        assert!(error.unwrap().contains("network failed"));
        assert!(!root.exists());
        let clients = temp.path.join("clients");
        let leftovers = fs::read_dir(clients)
            .map(|entries| entries.filter_map(Result::ok).count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn a_copy_without_a_version_is_not_installed() {
        let temp = TempDirectory::new("install-no-version");
        let (_, version, error, state, root) = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(false) },
        );
        assert_eq!(state, InstallState::Failed);
        assert_eq!(version, None);
        assert!(error.is_some());
        assert!(!root.exists());
    }

    #[test]
    fn a_failed_install_keeps_the_previous_copy() {
        let temp = TempDirectory::new("install-keep");
        let _ = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(true) },
        );
        let root = install_root(&temp.path, Client::Codex);
        fs::write(root.join("marker"), "old").expect("write marker");

        let (_, _, _, state, _) = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller {
                outcome: Err("network failed".to_owned()),
            },
        );
        assert_eq!(state, InstallState::Failed);
        assert_eq!(fs::read_to_string(root.join("marker")).unwrap(), "old");
    }

    #[cfg(unix)]
    #[test]
    fn absolute_install_links_are_moved_to_the_final_folder() {
        use std::os::unix::fs::symlink;

        let temp = TempDirectory::new("install-links");
        let staging = temp.path.join("staging");
        let root = temp.path.join("clients/claude-code");
        let real = staging.join("home/.local/share/claude/versions/1.0.0");
        fs::create_dir_all(real.parent().expect("version folder")).expect("create folders");
        fs::write(&real, "binary").expect("write binary");
        let launcher = staging.join("home/.local/bin/claude");
        fs::create_dir_all(launcher.parent().expect("launcher folder")).expect("create folders");
        symlink(&real, &launcher).expect("create launcher link");

        rebase_absolute_symlinks(&staging, &root).expect("rebase links");

        assert_eq!(
            fs::read_link(&launcher).expect("read launcher link"),
            root.join("home/.local/share/claude/versions/1.0.0")
        );
    }

    #[test]
    fn a_successful_install_replaces_the_previous_copy() {
        let temp = TempDirectory::new("install-replace");
        let _ = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(true) },
        );
        let root = install_root(&temp.path, Client::Codex);
        fs::write(root.join("marker"), "old").expect("write marker");

        let (_, _, _, state, _) = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(true) },
        );
        assert_eq!(state, InstallState::Done);
        assert!(!root.join("marker").exists());
    }

    #[test]
    fn an_update_swaps_the_app_copy_for_the_verified_copy() {
        let temp = TempDirectory::new("update-ok");
        let _ = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(true) },
        );
        let root = install_root(&temp.path, Client::Codex);
        fs::write(root.join("marker"), "old").expect("write marker");

        let (_, version, path, state, root) = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(true) },
        );
        assert_eq!(state, InstallState::Done);
        assert_eq!(version.as_deref(), Some("9.9.9"));
        assert!(path.is_some());
        assert!(!root.join("marker").exists(), "the old copy is replaced");
    }

    #[test]
    fn a_failed_update_keeps_the_old_copy_and_removes_staging() {
        let temp = TempDirectory::new("update-fail");
        let _ = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller { outcome: Ok(true) },
        );
        let root = install_root(&temp.path, Client::Codex);
        fs::write(root.join("marker"), "old").expect("write marker");

        let (_, version, error, state, root) = run_install_for_test(
            Client::Codex,
            &temp.path,
            &FakeInstaller {
                outcome: Err("network failed".to_owned()),
            },
        );
        assert_eq!(state, InstallState::Failed);
        assert_eq!(version, None);
        assert!(error.unwrap().contains("network failed"));
        assert_eq!(fs::read_to_string(root.join("marker")).unwrap(), "old");
        let leftovers = fs::read_dir(temp.path.join("clients"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_name() != "codex")
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "staging and backup folders are removed");
    }
}
