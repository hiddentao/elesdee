//! Detection of the local Tailscale client and its device name.
//!
//! The detector runs the CLI directly, never through a login shell. The
//! executable comes from `PATH` or a fixed install directory; no caller can
//! name a path. A read is cached for a short time so a burst of registrations
//! does not start a process each time.
//!
//! A missing, stopped or unreadable client gives no address. The caller keeps
//! the local address in every case.

use std::env;
use std::fmt;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// How long one status read stays usable.
const CACHE_TTL: Duration = Duration::from_secs(30);

/// How long the status command may run.
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);

/// How often the command is polled.
const STATUS_POLL: Duration = Duration::from_millis(25);

/// How long one Funnel change may run.
const FUNNEL_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a test client may run. Test executables finish at once; the
/// longer bound keeps a busy test machine from turning a slow start into a
/// failed read.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// HTTPS port that a Funnel address uses.
const FUNNEL_HTTPS_PORT: u16 = 443;

/// Maximum characters in a MagicDNS name.
const MAX_DNS_NAME_CHARS: usize = 253;

/// Maximum characters in a command reason shown to the user.
const MAX_REASON_CHARS: usize = 240;

/// The part of the device status that the app uses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Device {
    /// The MagicDNS name without its trailing dot, when it is usable.
    pub(crate) dns_name: Option<String>,
    /// The device's IPv4 address on the tailnet.
    pub(crate) ipv4: Option<Ipv4Addr>,
}

impl Device {
    /// Builds the address of a port on this device.
    ///
    /// The MagicDNS name gives an HTTPS address that stays the same when the
    /// address changes. Without it, the tailnet IPv4 address gives an HTTP
    /// address.
    pub(crate) fn address(&self, port: u16) -> Option<String> {
        if let Some(name) = &self.dns_name {
            return Some(format!("https://{name}:{port}"));
        }
        self.ipv4.map(|ip| format!("http://{ip}:{port}"))
    }
}

/// Why a Funnel change failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FunnelError {
    /// The client is missing, stopped or not signed in.
    Unavailable,
    /// The client refused the command.
    Command(String),
}

impl fmt::Display for FunnelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str(
                "Tailscale is not installed, not running or not signed in. The server stays local.",
            ),
            Self::Command(reason) => {
                write!(
                    formatter,
                    "Tailscale could not change the public address: {reason}"
                )
            }
        }
    }
}

/// The cached result of one status read.
struct Cached {
    read_at: Instant,
    device: Option<Device>,
}

/// Reads the local Tailscale status and caches it briefly.
pub(crate) struct Resolver {
    /// A fixed executable used by tests in place of discovery.
    binary: Option<PathBuf>,
    status_timeout: Duration,
    funnel_timeout: Duration,
    cache: Mutex<Option<Cached>>,
}

impl Resolver {
    pub(crate) fn new() -> Self {
        Self {
            binary: None,
            status_timeout: STATUS_TIMEOUT,
            funnel_timeout: FUNNEL_TIMEOUT,
            cache: Mutex::new(None),
        }
    }

    /// Uses one executable instead of discovery. Tests only.
    pub(crate) fn with_binary(binary: PathBuf) -> Self {
        Self {
            binary: Some(binary),
            status_timeout: TEST_TIMEOUT,
            funnel_timeout: TEST_TIMEOUT,
            cache: Mutex::new(None),
        }
    }

    /// Returns the tailnet address for a port, or nothing when Tailscale is
    /// not installed, not running or not readable.
    pub(crate) fn address(&self, port: u16) -> Option<String> {
        self.device().and_then(|device| device.address(port))
    }

    /// Makes a loopback port public through Tailscale Funnel.
    ///
    /// The command runs in the background so it returns after the client
    /// accepts it. Returns the public HTTPS address. The status is read fresh
    /// so a client that started after the last read is used. A missing client,
    /// a stopped backend or a refused command returns an error and changes
    /// nothing.
    pub(crate) fn enable_funnel(&self, port: u16) -> Result<String, FunnelError> {
        let device = self.fresh_device().ok_or(FunnelError::Unavailable)?;
        let name = device.dns_name.ok_or(FunnelError::Unavailable)?;
        let binary = self.binary_path().ok_or(FunnelError::Unavailable)?;
        run_funnel(
            &binary,
            &["funnel", "--bg", &port.to_string()],
            self.funnel_timeout,
        )?;
        Ok(format!("https://{name}/"))
    }

    /// Turns off the public address for a loopback port.
    pub(crate) fn disable_funnel(&self, port: u16) -> Result<(), FunnelError> {
        let binary = self.binary_path().ok_or(FunnelError::Unavailable)?;
        let target = format!("localhost:{port}");
        run_funnel(
            &binary,
            &[
                "funnel",
                &format!("--https={FUNNEL_HTTPS_PORT}"),
                &target,
                "off",
            ],
            self.funnel_timeout,
        )?;
        Ok(())
    }

    fn binary_path(&self) -> Option<PathBuf> {
        match &self.binary {
            Some(path) => Some(path.clone()),
            None => find_cli(),
        }
    }

    fn device(&self) -> Option<Device> {
        {
            let cache = self.lock_cache();
            if let Some(entry) = cache.as_ref()
                && entry.read_at.elapsed() < CACHE_TTL
            {
                return entry.device.clone();
            }
        }
        let device = self.read();
        self.cache_device(device.clone());
        device
    }

    fn fresh_device(&self) -> Option<Device> {
        let device = self.read();
        self.cache_device(device.clone());
        device
    }

    fn cache_device(&self, device: Option<Device>) {
        *self.lock_cache() = Some(Cached {
            read_at: Instant::now(),
            device,
        });
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, Option<Cached>> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn read(&self) -> Option<Device> {
        let binary = self.binary_path()?;
        let output = run_status(&binary, self.status_timeout)?;
        parse_status(&output)
    }
}

/// Finds the Tailscale CLI in `PATH` and then in the usual install locations.
fn find_cli() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            if directory.is_absolute() {
                candidates.push(directory.join(executable_name()));
            }
        }
    }
    for directory in fixed_directories() {
        candidates.push(directory.join(executable_name()));
    }
    candidates.into_iter().find_map(usable_executable)
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "tailscale.exe"
    } else {
        "tailscale"
    }
}

fn fixed_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    #[cfg(target_os = "macos")]
    {
        directories.push(PathBuf::from("/Applications/Tailscale.app/Contents/MacOS"));
    }
    for directory in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/opt/local/bin",
        "/snap/bin",
    ] {
        directories.push(PathBuf::from(directory));
    }
    #[cfg(windows)]
    {
        for directory in [
            r"C:\Program Files\Tailscale",
            r"C:\Program Files (x86)\Tailscale",
        ] {
            directories.push(PathBuf::from(directory));
        }
    }
    directories
}

fn usable_executable(path: PathBuf) -> Option<PathBuf> {
    if !crate::clients::is_executable(&path) {
        return None;
    }
    path.canonicalize().ok()
}

/// Runs the read-only status command and returns its standard output.
fn run_status(binary: &Path, timeout: Duration) -> Option<Vec<u8>> {
    let mut child = Command::new(binary)
        .arg("status")
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                return status.success().then_some(output.stdout);
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(STATUS_POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Runs one Funnel command and returns its standard output.
fn run_funnel(
    binary: &Path,
    arguments: &[&str],
    timeout: Duration,
) -> Result<Vec<u8>, FunnelError> {
    let mut child = Command::new(binary)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| FunnelError::Unavailable)?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|_| FunnelError::Unavailable)?;
                if status.success() {
                    return Ok(output.stdout);
                }
                return Err(FunnelError::Command(command_reason(&output.stderr)));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(STATUS_POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FunnelError::Command(
                    "Tailscale did not answer in time.".to_owned(),
                ));
            }
        }
    }
}

/// Turns the client's error output into one short user-facing sentence.
fn command_reason(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .and_then(|line| crate::text::clean_label(line, MAX_REASON_CHARS))
        .unwrap_or_else(|| "Tailscale refused the request.".to_owned())
}

#[derive(Deserialize)]
struct Status {
    #[serde(rename = "BackendState")]
    backend_state: Option<String>,
    #[serde(rename = "Self")]
    device: Option<SelfStatus>,
}

#[derive(Deserialize)]
struct SelfStatus {
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
    #[serde(rename = "TailscaleIPs", default)]
    addresses: Vec<String>,
}

/// Reads the device from the JSON that `tailscale status --json` prints.
pub(crate) fn parse_status(output: &[u8]) -> Option<Device> {
    let status: Status = serde_json::from_slice(output).ok()?;
    if status.backend_state.as_deref() != Some("Running") {
        return None;
    }
    let device = status.device?;
    let dns_name = device.dns_name.as_deref().and_then(clean_dns_name);
    let ipv4 = device
        .addresses
        .iter()
        .find_map(|address| tailnet_ipv4(address));
    if dns_name.is_none() && ipv4.is_none() {
        return None;
    }
    Some(Device { dns_name, ipv4 })
}

/// Accepts only a well-formed MagicDNS name in the shared tailnet domain.
fn clean_dns_name(value: &str) -> Option<String> {
    let name = value.trim_end_matches('.');
    if name.is_empty() || name.len() > MAX_DNS_NAME_CHARS {
        return None;
    }
    if !name.to_ascii_lowercase().ends_with(".ts.net") {
        return None;
    }
    let valid = name.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    valid.then(|| name.to_owned())
}

/// Accepts only an IPv4 address in the tailnet range 100.64.0.0/10.
fn tailnet_ipv4(value: &str) -> Option<Ipv4Addr> {
    let address: Ipv4Addr = value.parse().ok()?;
    let octets = address.octets();
    (octets[0] == 100 && octets[1] & 0xc0 == 64).then_some(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn status(backend: &str, dns: &str, addresses: &[&str]) -> Vec<u8> {
        serde_json::json!({
            "BackendState": backend,
            "Self": {
                "DNSName": dns,
                "TailscaleIPs": addresses,
            }
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn a_running_device_gives_its_magic_dns_address() {
        let device = parse_status(&status(
            "Running",
            "studio.tailnet.ts.net.",
            &["100.101.102.103", "fd7a:115c:a1e0::1"],
        ))
        .expect("device");
        assert_eq!(device.dns_name.as_deref(), Some("studio.tailnet.ts.net"));
        assert_eq!(device.ipv4, Some(Ipv4Addr::new(100, 101, 102, 103)));
        assert_eq!(
            device.address(3000).as_deref(),
            Some("https://studio.tailnet.ts.net:3000")
        );
    }

    #[test]
    fn a_device_without_a_magic_dns_name_uses_the_tailnet_address() {
        let device = parse_status(&status("Running", "", &["100.64.0.1"])).expect("device");
        assert_eq!(device.dns_name, None);
        assert_eq!(
            device.address(8080).as_deref(),
            Some("http://100.64.0.1:8080")
        );
    }

    #[test]
    fn a_name_outside_the_shared_domain_is_ignored() {
        let device = parse_status(&status("Running", "studio.example.com.", &["100.64.1.2"]))
            .expect("device");
        assert_eq!(device.dns_name, None);
        assert_eq!(
            device.address(3000).as_deref(),
            Some("http://100.64.1.2:3000")
        );
    }

    #[test]
    fn an_address_outside_the_tailnet_range_is_ignored() {
        assert_eq!(
            parse_status(&status("Running", "", &["100.63.255.255"])),
            None
        );
        assert_eq!(parse_status(&status("Running", "", &["100.128.0.0"])), None);
        assert_eq!(parse_status(&status("Running", "", &["192.168.1.2"])), None);
        assert!(parse_status(&status("Running", "", &["100.127.255.255"])).is_some());
    }

    #[test]
    fn a_stopped_or_unreadable_status_gives_no_device() {
        assert_eq!(parse_status(&status("Stopped", "", &["100.64.0.1"])), None);
        assert_eq!(
            parse_status(&status("NeedsLogin", "studio.tailnet.ts.net.", &[])),
            None
        );
        assert_eq!(parse_status(b"not json"), None);
        assert_eq!(parse_status(b"{}"), None);
    }

    #[test]
    fn a_malformed_name_is_rejected() {
        for name in [
            "-bad.tailnet.ts.net",
            "bad-.tailnet.ts.net",
            "bad..tailnet.ts.net",
            ".ts.net",
        ] {
            assert_eq!(clean_dns_name(name), None, "{name}");
        }
    }

    #[cfg(unix)]
    struct FakeCli {
        folder: PathBuf,
        binary: PathBuf,
    }

    #[cfg(unix)]
    impl FakeCli {
        fn new(label: &str, body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;

            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let folder = env::temp_dir().join(format!(
                "elesdee-tailscale-{label}-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir_all(&folder).expect("create folder");
            let binary = folder.join("tailscale");
            fs::write(&binary, format!("#!/bin/sh\n{body}\n")).expect("write script");
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
                .expect("make executable");
            Self { folder, binary }
        }
    }

    #[cfg(unix)]
    impl Drop for FakeCli {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_resolver_reads_a_fake_client_and_caches_it() {
        let calls = env::temp_dir().join(format!(
            "elesdee-tailscale-calls-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let calls_text = calls.to_string_lossy().into_owned();
        let cli = FakeCli::new(
            "running",
            &format!(
                "printf 'call\\n' >> '{calls_text}'\nprintf '{{\"BackendState\":\"Running\",\"Self\":{{\"DNSName\":\"studio.tailnet.ts.net.\",\"TailscaleIPs\":[\"100.100.100.100\"]}}}}'\n"
            ),
        );
        let resolver = Resolver::with_binary(cli.binary.clone());

        assert_eq!(
            resolver.address(4000).as_deref(),
            Some("https://studio.tailnet.ts.net:4000")
        );
        assert_eq!(
            resolver.address(5000).as_deref(),
            Some("https://studio.tailnet.ts.net:5000")
        );
        let written = fs::read_to_string(&calls).expect("read calls");
        assert_eq!(written.lines().count(), 1, "the status read is cached");
        let _ = fs::remove_file(&calls);
    }

    #[cfg(unix)]
    fn calls_path(label: &str) -> (PathBuf, String) {
        let path = env::temp_dir().join(format!(
            "elesdee-tailscale-funnel-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let text = path.to_string_lossy().into_owned();
        (path, text)
    }

    #[cfg(unix)]
    const RUNNING_STATUS: &str = "printf '{\"BackendState\":\"Running\",\"Self\":{\"DNSName\":\"studio.tailnet.ts.net.\",\"TailscaleIPs\":[\"100.100.100.100\"]}}'";

    #[cfg(unix)]
    #[test]
    fn funnel_is_enabled_for_a_registered_port() {
        let (calls, calls_text) = calls_path("enable");
        let cli = FakeCli::new(
            "funnel-enable",
            &format!(
                "case \"$1\" in\nstatus) {RUNNING_STATUS} ;;\nfunnel) printf '%s\\n' \"$*\" >> '{calls_text}' ;;\nesac"
            ),
        );
        let resolver = Resolver::with_binary(cli.binary.clone());

        let address = resolver.enable_funnel(3000).expect("public address");
        assert_eq!(address, "https://studio.tailnet.ts.net/");
        let written = fs::read_to_string(&calls).expect("read calls");
        assert_eq!(written.lines().collect::<Vec<_>>(), ["funnel --bg 3000"]);
        let _ = fs::remove_file(&calls);
    }

    #[cfg(unix)]
    #[test]
    fn funnel_needs_a_running_client() {
        let (calls, calls_text) = calls_path("stopped");
        let cli = FakeCli::new(
            "funnel-stopped",
            &format!(
                "case \"$1\" in\nstatus) printf '{{\"BackendState\":\"Stopped\",\"Self\":{{\"DNSName\":\"studio.tailnet.ts.net.\",\"TailscaleIPs\":[]}}}}' ;;\nfunnel) printf '%s\\n' \"$@\" >> '{calls_text}' ;;\nesac"
            ),
        );
        let resolver = Resolver::with_binary(cli.binary.clone());

        assert_eq!(resolver.enable_funnel(3000), Err(FunnelError::Unavailable));
        assert!(
            !calls.exists(),
            "the funnel command ran without a running client"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_funnel_command_keeps_its_reason() {
        let cli = FakeCli::new(
            "funnel-refused",
            &format!(
                "case \"$1\" in\nstatus) {RUNNING_STATUS} ;;\nfunnel) echo 'Funnel is not enabled for your tailnet.' >&2; exit 1 ;;\nesac"
            ),
        );
        let resolver = Resolver::with_binary(cli.binary.clone());

        assert_eq!(
            resolver.enable_funnel(3000),
            Err(FunnelError::Command(
                "Funnel is not enabled for your tailnet.".to_owned()
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn funnel_is_turned_off_for_a_registered_port() {
        let (calls, calls_text) = calls_path("disable");
        let cli = FakeCli::new(
            "funnel-disable",
            &format!(
                "case \"$1\" in\nstatus) {RUNNING_STATUS} ;;\nfunnel) printf '%s\\n' \"$*\" >> '{calls_text}' ;;\nesac"
            ),
        );
        let resolver = Resolver::with_binary(cli.binary.clone());

        resolver.disable_funnel(4000).expect("turn off");
        let written = fs::read_to_string(&calls).expect("read calls");
        assert_eq!(
            written.lines().collect::<Vec<_>>(),
            ["funnel --https=443 localhost:4000 off"]
        );
        let _ = fs::remove_file(&calls);
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_client_cannot_change_funnel() {
        let resolver = Resolver::with_binary(PathBuf::from("/nonexistent/tailscale"));
        assert_eq!(resolver.enable_funnel(3000), Err(FunnelError::Unavailable));
        assert_eq!(resolver.disable_funnel(3000), Err(FunnelError::Unavailable));
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_or_failing_client_gives_no_address() {
        let missing = Resolver::with_binary(PathBuf::from("/nonexistent/tailscale"));
        assert_eq!(missing.address(3000), None);

        let failing = FakeCli::new("failing", "echo \"$1 is not available\" >&2\nexit 1");
        assert_eq!(
            Resolver::with_binary(failing.binary.clone()).address(3000),
            None
        );
    }
}
