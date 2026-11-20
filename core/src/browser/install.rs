//! Finds, downloads and installs the browser the core runs.
//!
//! An installed Chrome or Chromium is used as it is. When none exists, the
//! pinned Chrome for Testing build for this computer's architecture is
//! downloaded from its official source, checked against pinned and published
//! digests, unpacked with path checks into a private data-directory folder and
//! marked executable. A version marker records the installed build, so the
//! next use reuses it and deleting the folder makes the next use download
//! again.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use data_encoding::{BASE64, HEXLOWER};
use futures::StreamExt;
use md5::Md5;
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::BrowserError;

const PINNED_VERSION: &str = "158.0.8127.36";
const DOWNLOAD_BASE: &str = "https://storage.googleapis.com/chrome-for-testing-public";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const PROGRESS_STEP: u64 = 256 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_SYMLINK_TARGET_BYTES: u64 = 4096;

/// The pinned Chrome for Testing build for one platform.
#[derive(Clone, Debug)]
pub(crate) struct PinnedBrowser {
    pub version: String,
    pub platform: String,
    pub url: String,
    pub sha256: String,
}

/// What the installer needs to find or fetch a browser.
#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub root: PathBuf,
    pub pinned: PinnedBrowser,
    /// False in tests that must exercise the download path on a computer with
    /// an installed browser.
    pub use_system: bool,
}

/// The step the shared install is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Phase {
    Idle,
    Checking,
    Downloading,
    Verifying,
    Unpacking,
    Ready,
    Failed,
}

impl Phase {
    fn busy(self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading | Self::Verifying | Self::Unpacking
        )
    }
}

/// Where the ready browser came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Source {
    System,
    Download,
}

/// Progress and result of the shared browser install.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Setup {
    pub phase: Phase,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub version: Option<String>,
    pub source: Option<Source>,
    pub error: Option<String>,
}

impl Setup {
    pub(crate) fn idle() -> Self {
        Self {
            phase: Phase::Idle,
            downloaded: 0,
            total: None,
            version: None,
            source: None,
            error: None,
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.phase.busy()
    }
}

/// The shared, process-wide browser install.
#[derive(Clone)]
pub(crate) struct Installer {
    options: Option<Options>,
    state: Arc<Mutex<State>>,
}

struct State {
    setup: Setup,
    executable: Option<PathBuf>,
    running: bool,
}

impl Installer {
    pub(crate) fn new(options: Option<Options>) -> Self {
        Self {
            options,
            state: Arc::new(Mutex::new(State {
                setup: Setup::idle(),
                executable: None,
                running: false,
            })),
        }
    }

    pub(crate) fn snapshot(&self) -> Setup {
        self.lock().setup.clone()
    }

    /// Returns the browser binary to run, resolving it without a download when
    /// one is already present.
    pub(crate) fn executable(&self) -> Option<PathBuf> {
        {
            let mut state = self.lock();
            if let Some(path) = state.executable.clone() {
                if path.is_file() {
                    return Some(path);
                }
                state.executable = None;
                state.setup.phase = Phase::Idle;
                state.setup.source = None;
                state.setup.version = None;
            }
        }
        if self
            .options
            .as_ref()
            .is_none_or(|options| options.use_system)
            && let Some(path) = system_executable()
        {
            self.ready(path.clone(), Source::System, None);
            return Some(path);
        }
        let options = self.options.as_ref()?;
        let path = installed_executable(options)?;
        self.ready(
            path.clone(),
            Source::Download,
            Some(options.pinned.version.clone()),
        );
        Some(path)
    }

    /// Starts a download when no browser is present. It returns at once; the
    /// caller reads progress from the setup snapshot.
    pub(crate) fn begin(&self) -> Result<(), BrowserError> {
        if self.executable().is_some() {
            return Ok(());
        }
        let Some(options) = self.options.clone() else {
            return Err(BrowserError::Unavailable(
                "Chrome for Testing cannot be downloaded on this computer.".to_owned(),
            ));
        };
        {
            let mut state = self.lock();
            if state.running || state.setup.phase.busy() {
                return Ok(());
            }
            state.running = true;
            state.setup = Setup {
                phase: Phase::Checking,
                ..Setup::idle()
            };
        }
        let installer = self.clone();
        tokio::spawn(async move {
            let result = install_engine(&installer, &options).await;
            let mut state = installer.lock();
            state.running = false;
            match result {
                Ok(executable) => {
                    state.executable = Some(executable);
                    state.setup.phase = Phase::Ready;
                    state.setup.source = Some(Source::Download);
                    state.setup.version = Some(options.pinned.version.clone());
                    state.setup.error = None;
                }
                Err(message) => {
                    state.setup.phase = Phase::Failed;
                    state.setup.error = Some(message);
                }
            }
        });
        Ok(())
    }

    fn ready(&self, path: PathBuf, source: Source, version: Option<String>) {
        let mut state = self.lock();
        state.executable = Some(path);
        state.setup.phase = Phase::Ready;
        state.setup.source = Some(source);
        state.setup.version = version;
        state.setup.error = None;
    }

    fn progress(&self, phase: Phase, downloaded: u64, total: Option<u64>) {
        let mut state = self.lock();
        state.setup.phase = phase;
        state.setup.downloaded = downloaded;
        if total.is_some() {
            state.setup.total = total;
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// Returns the pinned build for this computer, or `None` when the app has no
/// pinned build for its platform.
pub(crate) fn pinned() -> Option<PinnedBrowser> {
    let (platform, sha256) = pinned_platform()?;
    Some(PinnedBrowser {
        version: PINNED_VERSION.to_owned(),
        platform: platform.to_owned(),
        url: format!("{DOWNLOAD_BASE}/{PINNED_VERSION}/{platform}/chrome-{platform}.zip"),
        sha256: sha256.to_owned(),
    })
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn pinned_platform() -> Option<(&'static str, &'static str)> {
    Some((
        "mac-arm64",
        "b86d3b393def1f87d369b022abff0a51acb015d6fcb79ddfd1c379c9d72fbbe2",
    ))
}

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn pinned_platform() -> Option<(&'static str, &'static str)> {
    Some((
        "mac-x64",
        "1390e9b1386a1fd8ec2c4940b9ff59cbbda71b896b85182d7efbd73c58213802",
    ))
}

#[cfg(not(all(
    target_os = "macos",
    any(target_arch = "aarch64", target_arch = "x86_64")
)))]
fn pinned_platform() -> Option<(&'static str, &'static str)> {
    None
}

fn system_executable() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CHROME").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    super::chromium::default_chromium_path()
}

fn installed_executable(options: &Options) -> Option<PathBuf> {
    let marker = fs::read_to_string(options.root.join("version")).ok()?;
    if marker.trim() != options.pinned.version {
        return None;
    }
    let executable = options
        .root
        .join(&options.pinned.version)
        .join(relative_executable(&options.pinned.platform));
    executable.is_file().then_some(executable)
}

fn relative_executable(platform: &str) -> PathBuf {
    Path::new(&format!("chrome-{platform}"))
        .join("Google Chrome for Testing.app")
        .join("Contents")
        .join("MacOS")
        .join("Google Chrome for Testing")
}

async fn install_engine(installer: &Installer, options: &Options) -> Result<PathBuf, String> {
    if options.use_system
        && let Some(path) = system_executable()
    {
        return Ok(path);
    }
    if let Some(path) = installed_executable(options) {
        return Ok(path);
    }
    crate::private_fs::create_private_dir_all(&options.root)
        .map_err(|error| format!("The browser folder could not be created: {error}"))?;
    remove_stale_staging(&options.root);
    let staging = options.root.join(format!(".install-{}", Uuid::new_v4()));
    crate::private_fs::create_private_dir_all(&staging)
        .map_err(|error| format!("The browser folder could not be created: {error}"))?;
    let result = install_into(installer, options, &staging).await;
    let _ = fs::remove_dir_all(&staging);
    result
}

async fn install_into(
    installer: &Installer,
    options: &Options,
    staging: &Path,
) -> Result<PathBuf, String> {
    let archive = staging.join("browser.zip");
    installer.progress(Phase::Downloading, 0, None);
    download(
        &options.pinned.url,
        &archive,
        &options.pinned.sha256,
        installer,
    )
    .await?;

    installer.progress(Phase::Unpacking, 0, None);
    let unpacked = staging.join("browser");
    let archive_for_unpack = archive.clone();
    let target = unpacked.clone();
    tokio::task::spawn_blocking(move || unpack_archive(&archive_for_unpack, &target))
        .await
        .map_err(|error| format!("The browser archive could not be unpacked: {error}"))??;

    let installed = options.root.join(&options.pinned.version);
    if installed.exists() {
        fs::remove_dir_all(&installed)
            .map_err(|error| format!("The old browser folder could not be removed: {error}"))?;
    }
    fs::rename(&unpacked, &installed)
        .map_err(|error| format!("The browser could not be installed: {error}"))?;

    let executable = installed.join(relative_executable(&options.pinned.platform));
    if !executable.is_file() {
        return Err("The browser archive did not contain the expected program.".to_owned());
    }
    set_executable(&executable)?;
    crate::private_fs::write_private_file_atomic(
        &options.root.join("version"),
        options.pinned.version.as_bytes(),
    )
    .map_err(|error| format!("The browser version could not be recorded: {error}"))?;
    Ok(executable)
}

async fn download(
    url: &str,
    destination: &Path,
    expected_sha256: &str,
    installer: &Installer,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|error| format!("The browser download could not start: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("The browser download could not start: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "The browser download failed with status {}.",
            response.status()
        ));
    }
    let total = response.content_length();
    let published_md5 = published_md5(response.headers())?;
    let mut file = crate::private_fs::create_private_file(destination)
        .map_err(|error| format!("The browser download could not be saved: {error}"))?;
    let mut sha256 = Sha256::new();
    let mut md5 = Md5::new();
    let mut downloaded = 0u64;
    let mut reported = 0u64;
    installer.progress(Phase::Downloading, 0, total);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("The browser download stopped: {error}"))?;
        file.write_all(&chunk)
            .map_err(|error| format!("The browser download could not be saved: {error}"))?;
        sha256.update(&chunk);
        md5.update(&chunk);
        downloaded += chunk.len() as u64;
        if downloaded - reported >= PROGRESS_STEP {
            reported = downloaded;
            installer.progress(Phase::Downloading, downloaded, total);
        }
    }
    file.sync_all()
        .map_err(|error| format!("The browser download could not be saved: {error}"))?;
    installer.progress(Phase::Verifying, downloaded, total);
    let actual_sha256 = hex(sha256.finalize());
    let actual_md5 = hex(md5.finalize());
    check_digests(
        expected_sha256,
        published_md5.as_deref(),
        &actual_sha256,
        &actual_md5,
    )
}

fn check_digests(
    expected_sha256: &str,
    published_md5: Option<&str>,
    actual_sha256: &str,
    actual_md5: &str,
) -> Result<(), String> {
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(
            "The downloaded browser failed its pinned checksum check, so the app removed it."
                .to_owned(),
        );
    }
    if let Some(expected_md5) = published_md5
        && !actual_md5.eq_ignore_ascii_case(expected_md5)
    {
        return Err(
            "The downloaded browser does not match the checksum published with it, so the app removed it."
                .to_owned(),
        );
    }
    Ok(())
}

fn published_md5(headers: &reqwest::header::HeaderMap) -> Result<Option<String>, String> {
    for value in headers.get_all("x-goog-hash").iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let Some((name, encoded)) = value.split_once('=') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("md5") {
            continue;
        }
        let bytes = BASE64
            .decode(encoded.trim().as_bytes())
            .map_err(|_| "The download server published an invalid checksum.".to_owned())?;
        return Ok(Some(HEXLOWER.encode(&bytes)));
    }
    Ok(None)
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    HEXLOWER.encode(bytes.as_ref())
}

fn remove_stale_staging(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(".install-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("The browser program could not be made executable: {error}"))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Unpacks an archive below `target`, rejecting entries that escape it.
fn unpack_archive(archive_path: &Path, target: &Path) -> Result<(), String> {
    crate::private_fs::create_private_dir_all(target)
        .map_err(|error| format!("The browser folder could not be created: {error}"))?;
    let archive = File::open(archive_path)
        .map_err(|error| format!("The browser archive could not be opened: {error}"))?;
    let mut archive = zip::ZipArchive::new(archive)
        .map_err(|error| format!("The browser archive could not be read: {error}"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("The browser archive has too many entries.".to_owned());
    }
    let mut links = Vec::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("The browser archive could not be read: {error}"))?;
        let name = entry
            .name()
            .map_err(|error| format!("The browser archive has an invalid entry: {error}"))?
            .into_owned();
        let relative = safe_member_path(&name)?;
        if entry.is_symlink() {
            links.push((relative, read_link_target(&mut entry)?));
            continue;
        }
        let destination = target.join(&relative);
        if entry.is_dir() {
            crate::private_fs::create_private_dir_all(&destination)
                .map_err(|error| format!("The browser archive could not be unpacked: {error}"))?;
            continue;
        }
        let remaining = MAX_UNPACKED_BYTES
            .checked_sub(total)
            .ok_or_else(|| "The browser archive is too large.".to_owned())?;
        if entry.size() > remaining {
            return Err("The browser archive is too large.".to_owned());
        }
        if let Some(parent) = destination.parent() {
            crate::private_fs::create_private_dir_all(parent)
                .map_err(|error| format!("The browser archive could not be unpacked: {error}"))?;
        }
        let mut file = File::create(&destination)
            .map_err(|error| format!("The browser archive could not be unpacked: {error}"))?;
        let copied = std::io::copy(&mut entry.by_ref().take(remaining + 1), &mut file)
            .map_err(|error| format!("The browser archive could not be unpacked: {error}"))?;
        if copied > remaining {
            return Err("The browser archive is too large.".to_owned());
        }
        total += copied;
        if let Some(mode) = entry.unix_mode() {
            set_mode(&destination, mode & 0o777)?;
        }
    }
    for (relative, link) in links {
        create_link(target, &relative, &link)?;
    }
    Ok(())
}

fn read_link_target<R: Read>(entry: &mut zip::read::ZipFile<'_, R>) -> Result<String, String> {
    let mut bytes = Vec::new();
    entry
        .take(MAX_SYMLINK_TARGET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("The browser archive has an invalid link: {error}"))?;
    if bytes.len() as u64 > MAX_SYMLINK_TARGET_BYTES {
        return Err("The browser archive has an invalid link.".to_owned());
    }
    String::from_utf8(bytes).map_err(|_| "The browser archive has an invalid link.".to_owned())
}

/// Accepts only a relative path whose parts stay inside the archive folder.
fn safe_member_path(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains('\0') || name.contains('\\') || name.contains(':') {
        return Err("The browser archive has an unsafe path.".to_owned());
    }
    let path = Path::new(name);
    if path.is_absolute() {
        return Err("The browser archive has an unsafe path.".to_owned());
    }
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("The browser archive has an unsafe path.".to_owned());
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err("The browser archive has an unsafe path.".to_owned());
    }
    Ok(relative)
}

/// Accepts only a relative link target whose resolved path stays inside the
/// archive folder.
fn safe_link_target(member: &Path, link: &str) -> Result<(), String> {
    if link.is_empty() || link.contains('\0') || link.contains('\\') || link.contains(':') {
        return Err("The browser archive has an unsafe link.".to_owned());
    }
    let target = Path::new(link);
    if target.is_absolute() {
        return Err("The browser archive has an unsafe link.".to_owned());
    }
    let base = member.parent().unwrap_or_else(|| Path::new(""));
    let mut depth: i32 = base
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count() as i32;
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err("The browser archive has an unsafe link.".to_owned());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("The browser archive has an unsafe link.".to_owned());
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_link(target: &Path, relative: &Path, link: &str) -> Result<(), String> {
    safe_link_target(relative, link)?;
    let destination = target.join(relative);
    if let Some(parent) = destination.parent() {
        crate::private_fs::create_private_dir_all(parent)
            .map_err(|error| format!("The browser archive could not be unpacked: {error}"))?;
    }
    std::os::unix::fs::symlink(link, &destination)
        .map_err(|error| format!("The browser archive could not be unpacked: {error}"))
}

#[cfg(not(unix))]
fn create_link(_target: &Path, relative: &Path, link: &str) -> Result<(), String> {
    safe_link_target(relative, link)?;
    Err("The browser archive has a link, which this computer cannot unpack.".to_owned())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("The browser archive could not be unpacked: {error}"))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn temporary_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("elesdee-{label}-{}", Uuid::new_v4()));
        crate::private_fs::create_private_dir_all(&root).expect("create the test folder");
        root
    }

    fn archive_with(entries: &[(&str, &[u8])], links: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, bytes) in entries {
            writer.start_file(*name, options).expect("add a file");
            writer.write_all(bytes).expect("write a file");
        }
        for (name, target) in links {
            writer
                .add_symlink(*name, *target, options)
                .expect("add a link");
        }
        writer.finish().expect("finish the archive").into_inner()
    }

    fn write_archive(root: &Path, bytes: &[u8]) -> PathBuf {
        let path = root.join("browser.zip");
        fs::write(&path, bytes).expect("write the archive");
        path
    }

    #[test]
    fn rejects_paths_that_escape_the_archive_folder() {
        for name in [
            "../evil",
            "a/../../evil",
            "/etc/passwd",
            "a\\..\\evil",
            "C:/evil",
            "C:\\evil",
            "a/\0/evil",
            "",
        ] {
            assert!(safe_member_path(name).is_err(), "accepted {name:?}");
        }
        assert_eq!(
            safe_member_path("chrome/Google Chrome for Testing.app/Contents")
                .expect("a normal path"),
            PathBuf::from("chrome/Google Chrome for Testing.app/Contents")
        );
        assert_eq!(
            safe_member_path("./chrome/run").expect("a path with a current folder"),
            PathBuf::from("chrome/run")
        );
    }

    #[test]
    fn rejects_links_that_escape_the_archive_folder() {
        let member = Path::new("chrome/app/link");
        assert!(safe_link_target(member, "run").is_ok());
        assert!(safe_link_target(member, "../other").is_ok());
        assert!(safe_link_target(member, "../../../etc/passwd").is_err());
        assert!(safe_link_target(member, "/etc/passwd").is_err());
        assert!(safe_link_target(member, "..\\..\\evil").is_err());
    }

    #[test]
    fn unpacks_a_safe_archive() {
        let root = temporary_root("unpack-safe");
        let bytes = archive_with(
            &[
                ("chrome/run", b"#!/bin/sh\n"),
                ("chrome/readme.txt", b"hello"),
            ],
            &[("chrome/current", "run")],
        );
        let archive = write_archive(&root, &bytes);
        let target = root.join("out");
        unpack_archive(&archive, &target).expect("unpack the archive");
        assert_eq!(
            fs::read_to_string(target.join("chrome/readme.txt")).expect("read the file"),
            "hello"
        );
        let link = target.join("chrome/current");
        assert!(link.symlink_metadata().expect("read the link").is_symlink());
        assert_eq!(
            fs::read_to_string(link).expect("read through the link"),
            "#!/bin/sh\n"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_an_archive_with_a_traversing_path() {
        let root = temporary_root("unpack-escape");
        let bytes = archive_with(&[("chrome/ok", b"ok"), ("../evil", b"evil")], &[]);
        let archive = write_archive(&root, &bytes);
        let target = root.join("out");
        assert!(unpack_archive(&archive, &target).is_err());
        assert!(!root.join("evil").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_an_archive_with_a_traversing_link() {
        let root = temporary_root("unpack-link");
        let bytes = archive_with(&[], &[("chrome/current", "../../../etc/passwd")]);
        let archive = write_archive(&root, &bytes);
        let target = root.join("out");
        assert!(unpack_archive(&archive, &target).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_a_checksum_mismatch() {
        let actual = hex(Sha256::digest(b"the browser archive"));
        let expected = hex(Sha256::digest(b"another archive"));
        let message = check_digests(&expected, None, &actual, "00").expect_err("a mismatch");
        assert!(message.contains("checksum"));

        let md5 = hex(Md5::digest(b"elsewhere"));
        let message =
            check_digests(&expected, Some(&md5), &expected, "ffff").expect_err("a mismatch");
        assert!(message.contains("published"));
    }

    #[test]
    fn accepts_matching_digests() {
        let sha = hex(Sha256::digest(b"the browser archive"));
        let md5 = hex(Md5::digest(b"the browser archive"));
        check_digests(&sha, Some(&md5), &sha, &md5).expect("matching digests");
        check_digests(&sha.to_uppercase(), None, &sha, &md5).expect("case does not matter");
    }

    #[test]
    fn reads_the_published_md5_header() {
        let digest = Md5::digest(b"the browser archive");
        let encoded = BASE64.encode(&digest);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            "x-goog-hash",
            reqwest::header::HeaderValue::from_static("crc32c=AAAAAA=="),
        );
        headers.append(
            "x-goog-hash",
            reqwest::header::HeaderValue::from_str(&format!("md5={encoded}")).expect("header"),
        );
        assert_eq!(
            published_md5(&headers).expect("read the checksum"),
            Some(hex(digest))
        );

        let mut invalid = reqwest::header::HeaderMap::new();
        invalid.append(
            "x-goog-hash",
            reqwest::header::HeaderValue::from_static("md5=not base64"),
        );
        assert!(published_md5(&invalid).is_err());
    }
}
