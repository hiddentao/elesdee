//! The phone UI package: comparison, download, verification, safe unpack and
//! the custom protocol that serves the stored build to the webview.
//!
//! The shell keeps its own screens. A package is loaded only when the computer
//! offers one, its versions fit this bridge and its archive matches the
//! manifest. Every failure leaves the shell screens in place.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use elesdee_link::{
    AppConnection, BRIDGE_VERSION, MAX_UI_PACKAGE_BYTES, Method, PROTOCOL_VERSION, Response,
};
use serde::{Deserialize, Serialize};
use tauri::http::{Request as HttpRequest, Response as HttpResponse, StatusCode, header};
use zip::ZipArchive;

/// The manifest schema this shell understands.
pub const UI_SCHEMA: u32 = 1;
/// Maximum number of entries in one package.
pub const MAX_UI_ENTRIES: usize = 4096;
/// Maximum total size of the extracted files.
pub const MAX_UI_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum size of one extracted file.
pub const MAX_UI_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum length of one entry name in bytes.
pub const MAX_UI_NAME_BYTES: usize = 512;

const CURRENT_FILE: &str = "current.json";
const INDEX_FILE: &str = "index.html";
/// The generated script that carries the bridge marker to the package.
const MARKER_FILE: &str = "__elesdee_bridge.js";
const PACKAGE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

/// The UI package manifest as the computer sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiManifest {
    /// The manifest schema.
    pub schema: u32,
    /// The archive hash, as `blake3:<hex>`.
    pub build_id: String,
    /// The app version that built the package.
    pub app_version: String,
    /// The link protocol version that the package uses.
    pub protocol_version: u8,
    /// The oldest phone bridge that the package needs.
    pub min_bridge_version: u32,
    /// The archive size in bytes.
    pub bytes: u64,
    /// The number of entries in the archive.
    pub files: u32,
}

/// The result of comparing the computer and package versions with this shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCompatibility {
    /// The package fits this shell.
    Ready,
    /// The computer uses another protocol version.
    UpdateComputer,
    /// This shell, or the package, needs a newer phone app.
    UpdatePhone,
}

/// Compares the computer and package versions with this shell.
pub fn check_versions(
    computer_protocol: u8,
    computer_min_bridge: u32,
    manifest: Option<&UiManifest>,
) -> UiCompatibility {
    if computer_protocol != PROTOCOL_VERSION {
        return UiCompatibility::UpdateComputer;
    }
    if computer_min_bridge > BRIDGE_VERSION {
        return UiCompatibility::UpdatePhone;
    }
    if let Some(manifest) = manifest
        && (manifest.schema != UI_SCHEMA
            || manifest.protocol_version != PROTOCOL_VERSION
            || manifest.min_bridge_version > BRIDGE_VERSION)
    {
        return UiCompatibility::UpdatePhone;
    }
    UiCompatibility::Ready
}

/// A failure to download, check or store a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiError {
    /// The computer offers no package.
    Missing,
    /// The archive or one of its files is over its limit.
    TooLarge,
    /// The archive does not match its manifest.
    Hash,
    /// An entry name leaves the package folder, or an entry is a link.
    Path,
    /// The package has too many entries, a duplicate name or no entry page.
    Entries,
    /// The computer refused the download.
    Download(String),
    /// The app-private folder could not be written.
    Storage(String),
}

impl UiError {
    /// Returns the command error code.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Missing => "not_found",
            Self::TooLarge => "too_large",
            Self::Hash => "invalid_hash",
            Self::Path | Self::Entries => "invalid_package",
            Self::Download(_) => "download",
            Self::Storage(_) => "storage",
        }
    }

    /// Returns a short diagnostic message. The shell text comes from the
    /// language catalogs.
    pub fn message(&self) -> String {
        match self {
            Self::Missing => "This computer has no phone interface.".to_owned(),
            Self::TooLarge => "The phone interface package is too large.".to_owned(),
            Self::Hash => "The phone interface package failed its hash check.".to_owned(),
            Self::Path => "The phone interface package has an unsafe path.".to_owned(),
            Self::Entries => "The phone interface package is not valid.".to_owned(),
            Self::Download(message) => format!("The phone interface download failed: {message}"),
            Self::Storage(message) => format!("The phone interface could not be stored: {message}"),
        }
    }
}

/// The UI package state that the shell screens show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiStatus {
    /// The computer endpoint id.
    pub computer_id: String,
    /// One of `none`, `ready`, `update_phone`, `update_computer` or `failed`.
    pub state: &'static str,
    /// The webview address of the ready package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The stable error code for a failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// A short diagnostic message. The screens use the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl UiStatus {
    fn simple(computer_id: &str, state: &'static str) -> Self {
        Self {
            computer_id: computer_id.to_owned(),
            state,
            url: None,
            code: None,
            message: None,
        }
    }

    fn ready(computer_id: &str, url: String) -> Self {
        Self {
            computer_id: computer_id.to_owned(),
            state: "ready",
            url: Some(url),
            code: None,
            message: None,
        }
    }

    fn failed(computer_id: &str, error: &UiError) -> Self {
        Self {
            computer_id: computer_id.to_owned(),
            state: "failed",
            url: None,
            code: Some(error.code().to_owned()),
            message: Some(error.message()),
        }
    }
}

#[derive(Debug, Clone)]
struct Installed {
    computer_id: String,
    build_id: String,
    computer_name: Option<String>,
    directory: PathBuf,
}

impl Installed {
    fn url(&self) -> String {
        package_url(&self.computer_id, &self.build_id)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentBuild {
    build_id: String,
    #[serde(default)]
    computer_name: Option<String>,
}

#[derive(Default)]
struct UiInner {
    installed: HashMap<String, Installed>,
    active: Option<Installed>,
    statuses: HashMap<String, UiStatus>,
}

/// The app-private store and server for phone UI packages.
#[derive(Clone)]
pub struct UiStore {
    root: PathBuf,
    inner: Arc<Mutex<UiInner>>,
    gate: Arc<tokio::sync::Mutex<()>>,
    temp_counter: Arc<AtomicU64>,
}

impl UiStore {
    /// Creates a store under the app data folder.
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            root: app_data_dir.join("computers"),
            inner: Arc::new(Mutex::new(UiInner::default())),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            temp_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Makes the stored build for one computer the served build.
    ///
    /// The computer whose page the shell opens is the one named here, so a
    /// background update for another computer never changes it. A failed or
    /// incompatible check keeps the shell screens instead of an older package.
    pub fn activate(&self, computer_id: &str) -> UiStatus {
        let cached = lock(&self.inner).statuses.get(computer_id).cloned();
        if let Some(status) = cached.filter(|status| status.state != "ready") {
            return status;
        }
        let installed = self.find_current(computer_id);
        let mut inner = lock(&self.inner);
        let Some(installed) = installed else {
            if inner
                .active
                .as_ref()
                .is_some_and(|active| active.computer_id == computer_id)
            {
                inner.active = None;
            }
            let status = UiStatus::simple(computer_id, "none");
            inner
                .statuses
                .insert(computer_id.to_owned(), status.clone());
            return status;
        };
        let status = UiStatus::ready(computer_id, installed.url());
        inner
            .installed
            .insert(computer_id.to_owned(), installed.clone());
        inner.active = Some(installed);
        inner
            .statuses
            .insert(computer_id.to_owned(), status.clone());
        status
    }

    /// Checks the versions, downloads a changed build once and installs it.
    ///
    /// An unchanged build on disk is reused without a download. Every failure
    /// keeps the previous build and returns a status the screens can show.
    pub async fn ensure(
        &self,
        computer_id: &str,
        computer_protocol: u8,
        computer_min_bridge: u32,
        manifest: Option<UiManifest>,
        connection: &AppConnection,
    ) -> UiStatus {
        match check_versions(computer_protocol, computer_min_bridge, manifest.as_ref()) {
            UiCompatibility::UpdateComputer => {
                return self.save_status(UiStatus::simple(computer_id, "update_computer"));
            }
            UiCompatibility::UpdatePhone => {
                return self.save_status(UiStatus::simple(computer_id, "update_phone"));
            }
            UiCompatibility::Ready => {}
        }
        let Some(manifest) = manifest else {
            return self.save_status(UiStatus::simple(computer_id, "none"));
        };

        let computer_name = connection.welcome().computer_name.clone();
        self.reuse_or_install(
            computer_id,
            &manifest,
            &computer_name,
            self.download_and_install(computer_id, &manifest, connection),
        )
        .await
    }

    /// Reuses a stored build or installs the offered one, one at a time.
    ///
    /// The gate covers the stored-build check as well as the install, so a
    /// background check cannot prune the temporary folder or the newly placed
    /// build of another update for the same computer.
    async fn reuse_or_install(
        &self,
        computer_id: &str,
        manifest: &UiManifest,
        computer_name: &str,
        install: impl std::future::Future<Output = Result<Installed, UiError>>,
    ) -> UiStatus {
        let _gate = self.gate.lock().await;
        if let Some(mut installed) = self.find_build(computer_id, &manifest.build_id) {
            installed.computer_name = Some(computer_name.to_owned());
            return self.use_build(computer_id, installed);
        }
        match install.await {
            Ok(installed) => self.use_build(computer_id, installed),
            Err(error) => self.save_status(UiStatus::failed(computer_id, &error)),
        }
    }

    /// Adopts an installed build as the computer's current build.
    ///
    /// A build check that runs in the background must not change the page that
    /// is open, so this never touches the served build. It records the build
    /// in `current.json` and keeps that build, the build before it and the
    /// build on screen.
    fn use_build(&self, computer_id: &str, installed: Installed) -> UiStatus {
        let status = UiStatus::ready(computer_id, installed.url());
        let served = lock(&self.inner)
            .active
            .as_ref()
            .filter(|active| active.computer_id == computer_id)
            .map(|active| active.build_id.clone());
        let previous = self.current_build_id(computer_id);
        if let Ok(ui_root) = self.ui_root(computer_id) {
            if let Err(error) = write_current(
                &ui_root,
                &installed.build_id,
                installed.computer_name.as_deref(),
            ) {
                tracing::warn!(
                    %computer_id,
                    error = %error.message(),
                    "could not record the current phone UI build"
                );
            }
            prune_builds(
                &ui_root.join("builds"),
                &installed.build_id,
                previous.as_deref(),
                served.as_deref(),
            );
        }
        let mut inner = lock(&self.inner);
        inner.installed.insert(computer_id.to_owned(), installed);
        inner
            .statuses
            .insert(computer_id.to_owned(), status.clone());
        status
    }

    /// Saves and returns a failure status for one computer.
    pub fn fail(&self, computer_id: &str, error: UiError) -> UiStatus {
        self.save_status(UiStatus::failed(computer_id, &error))
    }

    fn save_status(&self, status: UiStatus) -> UiStatus {
        lock(&self.inner)
            .statuses
            .insert(status.computer_id.clone(), status.clone());
        status
    }

    async fn download_and_install(
        &self,
        computer_id: &str,
        manifest: &UiManifest,
        connection: &AppConnection,
    ) -> Result<Installed, UiError> {
        let response = connection
            .request(Method::UiDownload, None, Vec::new())
            .await
            .map_err(|error| UiError::Download(error.to_string()))?;
        let (served, archive) = match response {
            Response::Ok { result, body } => {
                let served: UiManifest =
                    serde_json::from_value(result).map_err(|_| UiError::Hash)?;
                (served, body)
            }
            Response::Error(body) => {
                return Err(if body.code() == elesdee_link::ErrorCode::NotFound {
                    UiError::Missing
                } else if body.code() == elesdee_link::ErrorCode::TooLarge {
                    UiError::TooLarge
                } else {
                    UiError::Download(format!("{}: {}", body.code(), body.message()))
                });
            }
            Response::StreamStart => {
                return Err(UiError::Download(
                    "the UI package download streamed".to_owned(),
                ));
            }
        };
        verify_download(&archive, &served)?;
        if served.build_id != manifest.build_id {
            return Err(UiError::Hash);
        }

        let ui_root = self.ui_root(computer_id)?;
        let builds = ui_root.join("builds");
        std::fs::create_dir_all(&builds).map_err(storage)?;
        let counter = self.temp_counter.fetch_add(1, Ordering::Relaxed);
        let temp = builds.join(format!(".tmp-{counter}"));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).map_err(storage)?;

        let unpacked = tokio::task::spawn_blocking({
            let temp = temp.clone();
            let manifest = served.clone();
            move || unpack(&archive, &temp, &manifest)
        })
        .await
        .map_err(|_| UiError::Storage("the unpack task stopped".to_owned()))?;
        if let Err(error) = unpacked {
            let _ = std::fs::remove_dir_all(&temp);
            return Err(error);
        }

        let directory = place_build(&builds, &temp, &served.build_id)?;

        tracing::info!(
            computer = %computer_id.chars().take(8).collect::<String>(),
            build = %served.build_id,
            bytes = served.bytes,
            "installed a phone UI package"
        );
        Ok(Installed {
            computer_id: computer_id.to_owned(),
            build_id: served.build_id,
            computer_name: Some(connection.welcome().computer_name.clone()),
            directory,
        })
    }

    fn ui_root(&self, computer_id: &str) -> Result<PathBuf, UiError> {
        if computer_id.is_empty()
            || !computer_id.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            return Err(UiError::Storage(
                "the computer id is not a safe folder name".to_owned(),
            ));
        }
        Ok(self.root.join(computer_id).join("ui"))
    }

    fn current_build_id(&self, computer_id: &str) -> Option<String> {
        let ui_root = self.ui_root(computer_id).ok()?;
        read_current(&ui_root).map(|current| current.0)
    }

    /// Returns the stored name for one computer, when the shell has it.
    fn computer_name(&self, computer_id: &str) -> Option<String> {
        {
            let inner = lock(&self.inner);
            let name = inner
                .installed
                .get(computer_id)
                .and_then(|installed| installed.computer_name.clone())
                .or_else(|| {
                    inner
                        .active
                        .as_ref()
                        .filter(|active| active.computer_id == computer_id)
                        .and_then(|active| active.computer_name.clone())
                });
            if name.is_some() {
                return name;
            }
        }
        let ui_root = self.ui_root(computer_id).ok()?;
        read_current(&ui_root).and_then(|current| current.1)
    }

    fn find_current(&self, computer_id: &str) -> Option<Installed> {
        let ui_root = self.ui_root(computer_id).ok()?;
        let (build_id, computer_name) = read_current(&ui_root)?;
        self.find_build_with_name(computer_id, &build_id, computer_name)
    }

    fn find_build(&self, computer_id: &str, build_id: &str) -> Option<Installed> {
        self.find_build_with_name(computer_id, build_id, None)
    }

    /// Resolves the build path in a package URL to an installed build.
    ///
    /// The path is a content hash with a fixed prefix, so the result stays
    /// inside the named computer's build folder. A build that did not install
    /// has no entry page and is not found.
    fn find_build_by_path(
        &self,
        computer_id: &str,
        build_segment: &str,
        computer_name: Option<String>,
    ) -> Option<Installed> {
        let hash = build_segment.strip_prefix("blake3-")?;
        let build_id = format!("blake3:{hash}");
        self.find_build_with_name(computer_id, &build_id, computer_name)
    }

    fn find_build_with_name(
        &self,
        computer_id: &str,
        build_id: &str,
        computer_name: Option<String>,
    ) -> Option<Installed> {
        let ui_root = self.ui_root(computer_id).ok()?;
        let directory = ui_root.join("builds").join(build_dir_name(build_id).ok()?);
        directory.join(INDEX_FILE).is_file().then(|| Installed {
            computer_id: computer_id.to_owned(),
            build_id: build_id.to_owned(),
            computer_name,
            directory,
        })
    }

    /// Serves one request on the `elesdee` protocol.
    ///
    /// The path names the computer, so a page keeps its own build root when
    /// another computer is opened or updates in the background.
    pub fn handle(&self, request: HttpRequest<Vec<u8>>) -> HttpResponse<Vec<u8>> {
        let path = request.uri().path();
        let mut segments = path.split('/').filter(|segment| !segment.is_empty());
        if segments.next() != Some("ui") {
            return not_found_response();
        }
        let (Some(computer_id), Some(build_segment)) = (segments.next(), segments.next()) else {
            return not_found_response();
        };
        let Some(installed) =
            self.find_build_by_path(computer_id, build_segment, self.computer_name(computer_id))
        else {
            return not_found_response();
        };
        let rest = segments.collect::<Vec<_>>().join("/");
        if rest == MARKER_FILE {
            return marker_response(&installed);
        }
        let name: &str = if rest.is_empty() { INDEX_FILE } else { &rest };
        if name == INDEX_FILE {
            return serve_index(&installed);
        }
        serve_file(&installed.directory, name)
    }

    /// Removes one computer's stored builds and its status.
    pub fn forget(&self, computer_id: &str) {
        {
            let mut inner = lock(&self.inner);
            inner.installed.remove(computer_id);
            inner.statuses.remove(computer_id);
            if inner
                .active
                .as_ref()
                .is_some_and(|active| active.computer_id == computer_id)
            {
                inner.active = None;
            }
        }
        if let Ok(ui_root) = self.ui_root(computer_id) {
            let _ = std::fs::remove_dir_all(ui_root);
        }
    }
}

impl std::fmt::Debug for UiStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiStore").field("root", &self.root).finish()
    }
}

/// Returns the webview address for one stored build of one computer.
pub fn package_url(computer_id: &str, build_id: &str) -> String {
    let host = if cfg!(any(target_os = "android", target_os = "windows")) {
        "http://elesdee.localhost"
    } else {
        "elesdee://localhost"
    };
    format!("{host}/ui/{computer_id}/{}/", build_path(build_id))
}

/// Returns the URL path segment for one build id.
fn build_path(build_id: &str) -> String {
    match build_hash(build_id) {
        Some(hash) => format!("blake3-{hash}"),
        None => "invalid".to_owned(),
    }
}

/// Checks one archive entry name.
pub fn check_entry_name(name: &str) -> Result<(), UiError> {
    if name.is_empty() || name.len() > MAX_UI_NAME_BYTES {
        return Err(UiError::Entries);
    }
    if name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(UiError::Path);
    }
    let path = Path::new(name);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(UiError::Path);
    }
    Ok(())
}

/// Checks a downloaded archive against its manifest.
pub fn verify_download(archive: &[u8], manifest: &UiManifest) -> Result<(), UiError> {
    if manifest.schema != UI_SCHEMA || build_hash(&manifest.build_id).is_none() {
        return Err(UiError::Hash);
    }
    if archive.len() > MAX_UI_PACKAGE_BYTES || manifest.bytes > MAX_UI_PACKAGE_BYTES as u64 {
        return Err(UiError::TooLarge);
    }
    if archive.len() as u64 != manifest.bytes {
        return Err(UiError::Hash);
    }
    let expected = build_hash(&manifest.build_id).ok_or(UiError::Hash)?;
    if blake3::hash(archive).to_hex().as_str() != expected {
        return Err(UiError::Hash);
    }
    check_entries(archive, manifest)
}

fn check_entries(archive: &[u8], manifest: &UiManifest) -> Result<(), UiError> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).map_err(|_| UiError::Entries)?;
    if zip.len() != manifest.files as usize || zip.len() > MAX_UI_ENTRIES {
        return Err(UiError::Entries);
    }
    let mut names = HashSet::with_capacity(zip.len());
    let mut unpacked = 0u64;
    let mut has_index = false;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|_| UiError::Entries)?;
        let name = entry.name().map_err(|_| UiError::Entries)?.into_owned();
        check_entry_name(&name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(UiError::Entries);
        }
        if entry.is_symlink() {
            return Err(UiError::Path);
        }
        if entry.size() > MAX_UI_ENTRY_BYTES {
            return Err(UiError::TooLarge);
        }
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > MAX_UI_UNPACKED_BYTES {
            return Err(UiError::TooLarge);
        }
        if name == INDEX_FILE {
            has_index = true;
        }
    }
    if !has_index {
        return Err(UiError::Entries);
    }
    Ok(())
}

/// Extracts a checked archive into an empty folder.
///
/// Every entry name is checked again, a symbolic link is refused, and an
/// existing file is never replaced.
pub fn unpack(archive: &[u8], destination: &Path, manifest: &UiManifest) -> Result<(), UiError> {
    verify_download(archive, manifest)?;
    let mut zip = ZipArchive::new(Cursor::new(archive)).map_err(|_| UiError::Entries)?;
    let mut seen = HashSet::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| UiError::Entries)?;
        let name = entry.name().map_err(|_| UiError::Entries)?.into_owned();
        check_entry_name(&name)?;
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(UiError::Entries);
        }
        if entry.is_symlink() {
            return Err(UiError::Path);
        }
        if entry.is_dir() {
            std::fs::create_dir_all(destination.join(&name)).map_err(storage)?;
            continue;
        }
        let target = destination.join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(storage)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(storage)?;
        std::io::copy(&mut entry, &mut file).map_err(storage)?;
        file.flush().map_err(storage)?;
    }
    if !seen.contains(INDEX_FILE) {
        return Err(UiError::Entries);
    }
    Ok(())
}

fn build_dir_name(build_id: &str) -> Result<String, UiError> {
    let hash = build_hash(build_id).ok_or(UiError::Hash)?;
    Ok(format!("blake3-{hash}"))
}

fn build_hash(build_id: &str) -> Option<&str> {
    let hash = build_id.strip_prefix("blake3:")?;
    if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(hash)
    } else {
        None
    }
}

fn read_current(ui_root: &Path) -> Option<(String, Option<String>)> {
    let bytes = std::fs::read(ui_root.join(CURRENT_FILE)).ok()?;
    let current: CurrentBuild = serde_json::from_slice(&bytes).ok()?;
    build_hash(&current.build_id)?;
    Some((current.build_id, current.computer_name))
}

fn write_current(
    ui_root: &Path,
    build_id: &str,
    computer_name: Option<&str>,
) -> Result<(), UiError> {
    let current = CurrentBuild {
        build_id: build_id.to_owned(),
        computer_name: computer_name.map(str::to_owned),
    };
    let bytes =
        serde_json::to_vec(&current).map_err(|error| UiError::Storage(error.to_string()))?;
    let temporary = ui_root.join(format!("{CURRENT_FILE}.tmp"));
    std::fs::write(&temporary, bytes).map_err(storage)?;
    std::fs::rename(&temporary, ui_root.join(CURRENT_FILE)).map_err(storage)
}

/// Places an unpacked build folder and keeps the builds that may still be used.
///
/// A destination without an entry page is an incomplete earlier install, so it
/// is replaced instead of adopted.
fn place_build(builds: &Path, temp: &Path, build_id: &str) -> Result<PathBuf, UiError> {
    let directory = builds.join(build_dir_name(build_id)?);
    if directory.join(INDEX_FILE).is_file() {
        let _ = std::fs::remove_dir_all(temp);
        return Ok(directory);
    }
    if directory.exists() {
        if directory.is_dir() {
            std::fs::remove_dir_all(&directory).map_err(storage)?;
        } else {
            std::fs::remove_file(&directory).map_err(storage)?;
        }
    }
    std::fs::rename(temp, &directory).map_err(storage)?;
    Ok(directory)
}

/// Deletes every build folder except the current, the previous current and
/// the build on screen.
fn prune_builds(builds: &Path, keep: &str, previous: Option<&str>, served: Option<&str>) {
    let kept = [Some(keep), previous, served]
        .into_iter()
        .flatten()
        .filter_map(|build| build_dir_name(build).ok())
        .collect::<HashSet<_>>();
    let Ok(entries) = std::fs::read_dir(builds) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            let _ = std::fs::remove_dir_all(entry.path());
            continue;
        }
        if kept.contains(name.as_ref()) {
            continue;
        }
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

fn serve_file(directory: &Path, name: &str) -> HttpResponse<Vec<u8>> {
    let Some(bytes) = read_package_file(directory, name) else {
        return not_found_response();
    };
    package_response(
        bytes,
        content_type(name),
        "public, max-age=31536000, immutable",
    )
}

/// Serves the entry page with the bridge marker before its scripts.
fn serve_index(installed: &Installed) -> HttpResponse<Vec<u8>> {
    let Some(bytes) = read_package_file(&installed.directory, INDEX_FILE) else {
        return not_found_response();
    };
    let body = match String::from_utf8(bytes) {
        Ok(html) => {
            let prefix = format!(
                "/ui/{}/{}",
                installed.computer_id,
                build_path(&installed.build_id)
            );
            inject_marker(&html, &prefix).into_bytes()
        }
        Err(error) => error.into_bytes(),
    };
    package_response(body, "text/html; charset=utf-8", "no-store")
}

/// Serves the generated marker script for the active build.
fn marker_response(installed: &Installed) -> HttpResponse<Vec<u8>> {
    package_response(
        marker_script(installed),
        "text/javascript; charset=utf-8",
        "no-store",
    )
}

fn package_response(
    body: Vec<u8>,
    mime: &'static str,
    cache: &'static str,
) -> HttpResponse<Vec<u8>> {
    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header("Content-Security-Policy", PACKAGE_CSP)
        .body(body)
        .unwrap_or_else(|_| not_found_response())
}

/// Reads one file only when it resolves inside the build folder.
fn read_package_file(directory: &Path, name: &str) -> Option<Vec<u8>> {
    if check_entry_name(name).is_err() {
        return None;
    }
    let target = directory.join(name);
    let canonical_directory = std::fs::canonicalize(directory).ok()?;
    let canonical_target = std::fs::canonicalize(&target).ok()?;
    if !canonical_target.starts_with(&canonical_directory) {
        return None;
    }
    std::fs::read(canonical_target).ok()
}

/// Writes the bridge marker that selects the shell transport.
fn marker_script(installed: &Installed) -> Vec<u8> {
    let marker = serde_json::json!({
        "computerId": installed.computer_id,
        "computerName": installed.computer_name.clone().unwrap_or_default(),
        "bridgeVersion": BRIDGE_VERSION,
        "protocolVersion": PROTOCOL_VERSION,
    });
    format!("window.__ELESDEE_PHONE__ = {marker};\n").into_bytes()
}

/// Adds the marker script tag before the package's own scripts.
///
/// The marker prefix is the package path for its computer and build.
fn inject_marker(html: &str, build_prefix: &str) -> String {
    let Some(index) = marker_index(html) else {
        return html.to_owned();
    };
    let tag = format!("<script src=\"{build_prefix}/{MARKER_FILE}\"></script>");
    let mut output = String::with_capacity(html.len() + tag.len());
    output.push_str(&html[..index]);
    output.push_str(&tag);
    output.push_str(&html[index..]);
    output
}

fn marker_index(html: &str) -> Option<usize> {
    let lower = html.to_ascii_lowercase();
    for tag in ["head", "body"] {
        let pattern = format!("<{tag}");
        if let Some(start) = lower.find(&pattern) {
            let after = start + pattern.len();
            let boundary = matches!(
                html[after..].chars().next(),
                Some('>') | Some(' ') | Some('\t') | Some('\n') | Some('\r')
            );
            if boundary && let Some(offset) = html[start..].find('>') {
                return Some(start + offset + 1);
            }
        }
    }
    lower.find("</head>")
}

fn content_type(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

/// Returns the response for an unknown package or file.
pub fn not_found_response() -> HttpResponse<Vec<u8>> {
    HttpResponse::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(b"Not found".to_vec())
        .expect("the not-found response is valid")
}

fn storage(error: std::io::Error) -> UiError {
    UiError::Storage(error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(build_id: String, bytes: u64, files: u32) -> UiManifest {
        UiManifest {
            schema: UI_SCHEMA,
            build_id,
            app_version: "0.1.0".to_owned(),
            protocol_version: PROTOCOL_VERSION,
            min_bridge_version: BRIDGE_VERSION,
            bytes,
            files,
        }
    }

    fn sample_zip() -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file(INDEX_FILE, options).expect("index");
            writer.write_all(b"<!doctype html>").expect("write index");
            writer.start_file("assets/app.js", options).expect("asset");
            writer
                .write_all(b"console.log('phone');")
                .expect("write asset");
            writer.finish().expect("finish archive");
        }
        output.into_inner()
    }

    fn temp_folder(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "elesdee-phone-ui-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&path).expect("create temporary folder");
        path
    }

    #[test]
    fn a_shell_at_the_same_versions_is_ready() {
        let manifest = manifest(format!("blake3:{}", blake3::hash(b"").to_hex()), 0, 0);
        assert_eq!(
            check_versions(PROTOCOL_VERSION, BRIDGE_VERSION, Some(&manifest)),
            UiCompatibility::Ready
        );
        assert_eq!(
            check_versions(PROTOCOL_VERSION + 1, BRIDGE_VERSION, Some(&manifest)),
            UiCompatibility::UpdateComputer
        );
        assert_eq!(
            check_versions(PROTOCOL_VERSION, BRIDGE_VERSION, None),
            UiCompatibility::Ready
        );
    }

    #[test]
    fn a_newer_bridge_or_package_needs_a_phone_update() {
        assert_eq!(
            check_versions(PROTOCOL_VERSION, BRIDGE_VERSION + 1, None),
            UiCompatibility::UpdatePhone
        );
        let mut newer = manifest(format!("blake3:{}", blake3::hash(b"").to_hex()), 0, 0);
        newer.min_bridge_version = BRIDGE_VERSION + 1;
        assert_eq!(
            check_versions(PROTOCOL_VERSION, BRIDGE_VERSION, Some(&newer)),
            UiCompatibility::UpdatePhone
        );
        let mut other = manifest(format!("blake3:{}", blake3::hash(b"").to_hex()), 0, 0);
        other.protocol_version = PROTOCOL_VERSION + 1;
        assert_eq!(
            check_versions(PROTOCOL_VERSION, BRIDGE_VERSION, Some(&other)),
            UiCompatibility::UpdatePhone
        );
    }

    #[test]
    fn a_changed_archive_fails_the_hash_check() {
        let archive = sample_zip();
        let good = manifest(
            format!("blake3:{}", blake3::hash(&archive).to_hex()),
            archive.len() as u64,
            2,
        );
        assert_eq!(verify_download(&archive, &good), Ok(()));

        let mut changed = archive.clone();
        changed.push(0);
        assert_eq!(verify_download(&changed, &good), Err(UiError::Hash));
        let wrong_size = manifest(
            format!("blake3:{}", blake3::hash(&archive).to_hex()),
            archive.len() as u64 + 1,
            2,
        );
        assert_eq!(verify_download(&archive, &wrong_size), Err(UiError::Hash));
    }

    #[test]
    fn an_archive_over_the_cap_is_refused() {
        let archive = sample_zip();
        let big = manifest(
            format!("blake3:{}", blake3::hash(&archive).to_hex()),
            MAX_UI_PACKAGE_BYTES as u64 + 1,
            2,
        );
        assert_eq!(verify_download(&archive, &big), Err(UiError::TooLarge));
    }

    #[test]
    fn an_unsafe_entry_name_is_refused() {
        for name in ["../evil.js", "/etc/passwd", "assets\\evil.js", "a//b", ""] {
            assert!(check_entry_name(name).is_err(), "{name}");
        }
        assert!(check_entry_name("assets/app.js").is_ok());
    }

    #[test]
    fn a_traversal_entry_is_refused_before_extraction() {
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file(INDEX_FILE, options).expect("index");
            writer.write_all(b"<!doctype html>").expect("write index");
            writer.start_file("../outside.js", options).expect("evil");
            writer.write_all(b"alert(1)").expect("write evil");
            writer.finish().expect("finish archive");
        }
        let archive = output.into_inner();
        let manifest = manifest(
            format!("blake3:{}", blake3::hash(&archive).to_hex()),
            archive.len() as u64,
            2,
        );
        let folder = temp_folder("traversal");
        let result = unpack(&archive, &folder, &manifest);
        assert_eq!(result, Err(UiError::Path));
        assert!(!folder.join("outside.js").exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_package_extracts_into_its_folder() {
        let archive = sample_zip();
        let manifest = manifest(
            format!("blake3:{}", blake3::hash(&archive).to_hex()),
            archive.len() as u64,
            2,
        );
        let folder = temp_folder("unpack");
        unpack(&archive, &folder, &manifest).expect("unpack");
        assert_eq!(
            std::fs::read(folder.join(INDEX_FILE)).expect("index"),
            b"<!doctype html>"
        );
        assert!(folder.join("assets/app.js").is_file());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_served_build_uses_the_platform_origin() {
        let hash = blake3::hash(b"package").to_hex().to_string();
        let url = package_url("computer", &format!("blake3:{hash}"));
        assert!(url.contains(&format!("/ui/computer/blake3-{hash}/")));
        if cfg!(any(target_os = "android", target_os = "windows")) {
            assert!(url.starts_with("http://elesdee.localhost/"));
        } else {
            assert!(url.starts_with("elesdee://localhost/"));
        }
    }

    /// Creates one stored build below the store root and returns its folder.
    fn stored_build(
        folder: &Path,
        computer_id: &str,
        build_id: &str,
        index: &[u8],
        asset: &[u8],
    ) -> PathBuf {
        let directory = folder
            .join("computers")
            .join(computer_id)
            .join("ui")
            .join("builds")
            .join(build_dir_name(build_id).expect("build name"));
        std::fs::create_dir_all(directory.join("assets")).expect("build folder");
        std::fs::write(directory.join(INDEX_FILE), index).expect("index");
        std::fs::write(directory.join("assets/app.js"), asset).expect("asset");
        directory
    }

    /// Builds one installed record for a stored build folder.
    fn installed(computer_id: &str, build_id: &str, directory: PathBuf) -> Installed {
        Installed {
            computer_id: computer_id.to_owned(),
            build_id: build_id.to_owned(),
            computer_name: Some("Studio".to_owned()),
            directory,
        }
    }

    fn request(path: &str) -> HttpRequest<Vec<u8>> {
        HttpRequest::builder()
            .uri(path)
            .body(Vec::new())
            .expect("request")
    }

    #[test]
    fn the_protocol_serves_the_current_and_previous_builds() {
        let folder = temp_folder("serve");
        let store = UiStore::new(&folder);
        let current = format!("blake3:{}", "a".repeat(64));
        let previous = format!("blake3:{}", "b".repeat(64));
        let current_directory =
            stored_build(&folder, "computer", &current, b"current", b"current-js");
        stored_build(&folder, "computer", &previous, b"previous", b"previous-js");
        {
            let mut inner = lock(&store.inner);
            inner.active = Some(Installed {
                computer_id: "computer".to_owned(),
                build_id: current.clone(),
                computer_name: Some("Studio".to_owned()),
                directory: current_directory,
            });
        }

        let current_path = build_path(&current);
        let response = store.handle(request(&format!("/ui/computer/{current_path}/")));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"current");

        let previous_path = build_path(&previous);
        let response = store.handle(request(&format!("/ui/computer/{previous_path}/")));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"previous");
        let response = store.handle(request(&format!(
            "/ui/computer/{previous_path}/assets/app.js"
        )));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"previous-js");

        let unknown = build_path(&format!("blake3:{}", "c".repeat(64)));
        let response = store.handle(request(&format!("/ui/computer/{unknown}/")));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = store.handle(request(&format!(
            "/ui/computer/{previous_path}/../outside.js"
        )));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = store.handle(request(&format!(
            "/ui/computer/{previous_path}/assets/../../x.js"
        )));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = store.handle(request("/ui/not-a-build/"));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = store.handle(request("/ui/computer/not-a-build/"));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_protocol_refuses_a_build_stored_for_another_computer() {
        let folder = temp_folder("serve-other");
        let store = UiStore::new(&folder);
        let current = format!("blake3:{}", "a".repeat(64));
        let other = format!("blake3:{}", "d".repeat(64));
        stored_build(&folder, "computer", &current, b"current", b"js");
        stored_build(&folder, "other", &other, b"other", b"other-js");

        // The build path resolves only under the computer named before it.
        let response = store.handle(request(&format!("/ui/computer/{}/", build_path(&other))));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = store.handle(request(&format!("/ui/other/{}/", build_path(&current))));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_page_returns_to_the_first_computer_after_the_second() {
        let folder = temp_folder("serve-two");
        let store = UiStore::new(&folder);
        let first = format!("blake3:{}", "1".repeat(64));
        let second = format!("blake3:{}", "2".repeat(64));
        stored_build(&folder, "first", &first, b"first", b"first-js");
        let second_directory = stored_build(&folder, "second", &second, b"second", b"second-js");
        let first_root = store.ui_root("first").expect("first root");
        let second_root = store.ui_root("second").expect("second root");
        write_current(&first_root, &first, Some("First")).expect("current first");
        write_current(&second_root, &second, Some("Second")).expect("current second");

        let status = store.activate("first");
        assert_eq!(status.state, "ready");
        assert!(status.url.expect("url").contains("/ui/first/"));
        let status = store.activate("second");
        assert_eq!(status.state, "ready");
        assert!(status.url.expect("url").contains("/ui/second/"));

        // Opening the first computer again makes its build the served one.
        let status = store.activate("first");
        assert!(status.url.expect("url").contains("/ui/first/"));
        assert_eq!(
            lock(&store.inner)
                .active
                .as_ref()
                .expect("active")
                .computer_id,
            "first"
        );

        // A background update for the second computer leaves the page alone.
        store.use_build("second", installed("second", &second, second_directory));
        assert_eq!(
            lock(&store.inner)
                .active
                .as_ref()
                .expect("active")
                .computer_id,
            "first"
        );

        let path = build_path(&first);
        let response = store.handle(request(&format!("/ui/first/{path}/")));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"first");
        let response = store.handle(request(&format!("/ui/first/{path}/assets/app.js")));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"first-js");
        let response = store.handle(request(&format!("/ui/second/{path}/")));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn two_updates_keep_the_page_that_is_open() {
        let folder = temp_folder("serve-updates");
        let store = UiStore::new(&folder);
        let first = format!("blake3:{}", "a".repeat(64));
        let second = format!("blake3:{}", "b".repeat(64));
        let third = format!("blake3:{}", "c".repeat(64));
        let first_directory = stored_build(&folder, "computer", &first, b"first", b"first-js");
        let ui_root = store.ui_root("computer").expect("root");
        write_current(&ui_root, &first, Some("Studio")).expect("current first");
        assert_eq!(store.activate("computer").state, "ready");

        let second_directory = stored_build(&folder, "computer", &second, b"second", b"second-js");
        assert_eq!(
            store
                .use_build("computer", installed("computer", &second, second_directory))
                .state,
            "ready"
        );
        assert_eq!(
            lock(&store.inner)
                .active
                .as_ref()
                .expect("served build")
                .build_id,
            first
        );
        let third_directory = stored_build(&folder, "computer", &third, b"third", b"third-js");
        assert_eq!(
            store
                .use_build("computer", installed("computer", &third, third_directory))
                .state,
            "ready"
        );
        assert_eq!(
            lock(&store.inner)
                .active
                .as_ref()
                .expect("served build")
                .build_id,
            first
        );

        // The page opened before both updates still loads its own build.
        assert!(first_directory.join(INDEX_FILE).is_file());
        let response = store.handle(request(&format!(
            "/ui/computer/{}/assets/app.js",
            build_path(&first)
        )));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"first-js");
        assert_eq!(read_current(&ui_root).expect("current").0, third);

        let _ = std::fs::remove_dir_all(folder);
    }

    #[tokio::test]
    async fn two_concurrent_updates_keep_both_builds_and_the_page_build() {
        let folder = temp_folder("concurrent");
        let store = UiStore::new(&folder);
        let current = format!("blake3:{}", "a".repeat(64));
        let next = format!("blake3:{}", "b".repeat(64));
        let current_directory =
            stored_build(&folder, "computer", &current, b"current", b"current-js");
        let ui_root = store.ui_root("computer").expect("root");
        write_current(&ui_root, &current, Some("Studio")).expect("current");
        assert_eq!(store.activate("computer").state, "ready");

        let builds = ui_root.join("builds");
        let current_manifest = manifest(current.clone(), 0, 0);
        let next_manifest = manifest(next.clone(), 0, 0);
        let install_next = async {
            let temp = builds.join(".tmp-concurrent");
            std::fs::create_dir_all(&temp).map_err(storage)?;
            std::fs::write(temp.join(INDEX_FILE), b"next").map_err(storage)?;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let directory = place_build(&builds, &temp, &next)?;
            Ok(Installed {
                computer_id: "computer".to_owned(),
                build_id: next.clone(),
                computer_name: Some("Studio".to_owned()),
                directory,
            })
        };

        // One update installs a new build while the other reuses the build
        // that is already stored and on screen.
        let (next_status, current_status) = tokio::join!(
            store.reuse_or_install("computer", &next_manifest, "Studio", install_next),
            store.reuse_or_install("computer", &current_manifest, "Studio", async {
                unreachable!("the stored build is reused")
            },),
        );

        assert_eq!(next_status.state, "ready");
        assert_eq!(current_status.state, "ready");
        assert!(store.find_build("computer", &next).is_some());
        assert!(store.find_build("computer", &current).is_some());
        assert_eq!(read_current(&ui_root).expect("current").0, current);

        let page = store.handle(request(&format!("/ui/computer/{}/", build_path(&current))));
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(page.body(), b"current");
        let response = store.handle(request(&format!("/ui/computer/{}/", build_path(&next))));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"next");
        assert!(!builds.join(".tmp-concurrent").exists());
        assert!(current_directory.join(INDEX_FILE).is_file());

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn an_incomplete_build_folder_is_replaced() {
        let folder = temp_folder("partial");
        let builds = folder.join("builds");
        std::fs::create_dir_all(&builds).expect("builds");
        let build = format!("blake3:{}", "f".repeat(64));
        let directory = builds.join(build_dir_name(&build).expect("name"));
        std::fs::create_dir_all(&directory).expect("partial build");
        std::fs::write(directory.join("stale.js"), b"old").expect("stale");

        let temp = builds.join(".tmp-test");
        std::fs::create_dir_all(&temp).expect("temporary build");
        std::fs::write(temp.join(INDEX_FILE), b"<!doctype html>").expect("index");
        std::fs::write(temp.join("app.js"), b"new").expect("app");

        let placed = place_build(&builds, &temp, &build).expect("place");
        assert_eq!(placed, directory);
        assert_eq!(
            std::fs::read(directory.join(INDEX_FILE)).expect("index"),
            b"<!doctype html>"
        );
        assert_eq!(
            std::fs::read(directory.join("app.js")).expect("app"),
            b"new"
        );
        assert!(!directory.join("stale.js").exists());
        assert!(!temp.exists());

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_entry_page_loads_the_marker_before_its_scripts() {
        let folder = temp_folder("marker");
        let build = format!("blake3:{}", "e".repeat(64));
        let store = UiStore::new(&folder);
        let package = stored_build(
            &folder,
            "computer-1",
            &build,
            b"<!doctype html><html><head><script src=\"./assets/app.js\"></script></head><body></body></html>",
            b"app",
        );
        {
            let mut inner = lock(&store.inner);
            inner.active = Some(Installed {
                computer_id: "computer-1".to_owned(),
                build_id: build.clone(),
                computer_name: Some("Studio <desk>".to_owned()),
                directory: package,
            });
        }

        let path = build_path(&build);
        let html = String::from_utf8(
            store
                .handle(request(&format!("/ui/computer-1/{path}/")))
                .body()
                .clone(),
        )
        .expect("html");
        let marker = html
            .find(&format!("/ui/computer-1/{path}/{MARKER_FILE}"))
            .expect("marker");
        let script = html.find("./assets/app.js").expect("script");
        assert!(marker < script, "the marker runs before the package script");

        let response = store.handle(request(&format!("/ui/computer-1/{path}/{MARKER_FILE}")));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/javascript; charset=utf-8")
        );
        let script = String::from_utf8(response.body().clone()).expect("script");
        assert!(script.contains("window.__ELESDEE_PHONE__"));
        assert!(script.contains("computer-1"));
        assert!(script.contains("Studio <desk>"));
        assert!(script.contains(&format!("\"bridgeVersion\":{BRIDGE_VERSION}")));

        let response = store.handle(request(&format!(
            "/ui/computer-1/{}/{MARKER_FILE}",
            build_path(&format!("blake3:{}", blake3::hash(b"y").to_hex()))
        )));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_page_without_a_head_keeps_its_bytes() {
        let installed = Installed {
            computer_id: "computer".to_owned(),
            build_id: format!("blake3:{}", blake3::hash(b"x").to_hex()),
            computer_name: None,
            directory: PathBuf::new(),
        };
        assert_eq!(inject_marker("index", "/ui/computer/blake3-x"), "index");
        assert!(
            inject_marker("<html><body></body></html>", "/ui/computer/blake3-x")
                .contains("/ui/computer/blake3-x/")
        );
        assert!(
            marker_script(&installed)
                .windows(13)
                .any(|window| window == b"computerName\"")
        );
    }
}
