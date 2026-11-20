//! The phone UI package that a paired phone downloads.
//!
//! The desktop build writes a deterministic zip and a manifest. The core reads
//! them once at startup, checks the hash, the byte count and the archive entry
//! names, and caches the archive in memory. A package that fails the check is
//! not served. A missing package leaves pairing and the rest of the link
//! working; `ui.manifest` and `ui.download` then return a clear code.

use std::collections::HashSet;
use std::fmt;
use std::io::Cursor;
use std::path::{Component, Path};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use elesdee_link::{ErrorCode, MAX_UI_PACKAGE_BYTES};

/// The package manifest file name inside the package folder.
pub const UI_MANIFEST_FILE: &str = "phone-ui.json";
/// The package archive file name inside the package folder.
pub const UI_ARCHIVE_FILE: &str = "phone-ui.zip";
/// The manifest schema this core understands.
pub const UI_SCHEMA: u32 = 1;
/// Maximum number of entries in one package.
pub const MAX_UI_ENTRIES: usize = 4096;
/// Maximum total size of the extracted files.
pub const MAX_UI_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum size of one extracted file.
pub const MAX_UI_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum length of one archive entry name in bytes.
pub const MAX_UI_NAME_BYTES: usize = 512;
/// Maximum size of the manifest file.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// The manifest that describes one phone UI package.
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

/// A failure to load or serve the phone UI package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiPackageError {
    /// No manifest or archive exists at the package folder.
    Missing,
    /// The archive or one of its files is over its limit.
    TooLarge,
    /// The package is malformed or fails its hash check.
    Invalid(String),
}

impl UiPackageError {
    /// Returns the link error code for this failure.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Missing => ErrorCode::NotFound,
            Self::TooLarge => ErrorCode::TooLarge,
            Self::Invalid(_) => ErrorCode::Internal,
        }
    }

    /// Returns a message that the phone can show.
    pub fn message(&self) -> String {
        match self {
            Self::Missing => "This computer has no phone interface.".to_owned(),
            Self::TooLarge => "The phone interface package is over its size limit.".to_owned(),
            Self::Invalid(reason) => format!("The phone interface package is not valid: {reason}"),
        }
    }
}

impl fmt::Display for UiPackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for UiPackageError {}

/// A loaded phone UI package with its archive cached in memory.
#[derive(Debug)]
pub struct UiPackage {
    manifest: UiManifest,
    archive: Vec<u8>,
}

impl UiPackage {
    /// Reads the manifest and the archive from a package folder.
    ///
    /// The archive hash, the byte count, the entry count and every entry name
    /// are checked before the package is accepted.
    ///
    /// # Errors
    ///
    /// Returns [`UiPackageError::Missing`] when the manifest or the archive is
    /// absent, [`UiPackageError::TooLarge`] when a documented limit is passed,
    /// and [`UiPackageError::Invalid`] for a malformed package or a hash
    /// mismatch.
    pub fn load(directory: &Path) -> Result<Self, UiPackageError> {
        let manifest_path = directory.join(UI_MANIFEST_FILE);
        let archive_path = directory.join(UI_ARCHIVE_FILE);
        if !manifest_path.is_file() || !archive_path.is_file() {
            return Err(UiPackageError::Missing);
        }

        let manifest_bytes = std::fs::read(&manifest_path)
            .map_err(|error| UiPackageError::Invalid(format!("the manifest: {error}")))?;
        if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(UiPackageError::TooLarge);
        }
        let manifest: UiManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|error| UiPackageError::Invalid(format!("the manifest: {error}")))?;
        validate_manifest(&manifest)?;

        let metadata = std::fs::metadata(&archive_path)
            .map_err(|error| UiPackageError::Invalid(format!("the archive: {error}")))?;
        if metadata.len() > MAX_UI_PACKAGE_BYTES as u64 {
            return Err(UiPackageError::TooLarge);
        }
        let archive = std::fs::read(&archive_path)
            .map_err(|error| UiPackageError::Invalid(format!("the archive: {error}")))?;
        if archive.len() > MAX_UI_PACKAGE_BYTES {
            return Err(UiPackageError::TooLarge);
        }
        if archive.len() as u64 != manifest.bytes {
            return Err(UiPackageError::Invalid(
                "the archive size does not match the manifest".to_owned(),
            ));
        }
        let expected = build_hash(&manifest.build_id)
            .ok_or_else(|| UiPackageError::Invalid("the build id is not valid".to_owned()))?;
        if blake3::hash(&archive).to_hex().as_str() != expected {
            return Err(UiPackageError::Invalid(
                "the archive hash does not match the manifest".to_owned(),
            ));
        }
        validate_entries(&archive, &manifest)?;

        Ok(Self { manifest, archive })
    }

    /// Returns the package manifest.
    pub fn manifest(&self) -> &UiManifest {
        &self.manifest
    }

    /// Returns the cached archive bytes.
    pub fn archive(&self) -> &[u8] {
        &self.archive
    }
}

/// The package state that the link handlers use.
///
/// A missing package and a failed check are both kept, so the phone gets a
/// clear code instead of a silent empty result.
#[derive(Clone, Default)]
pub struct UiPackageState {
    package: Option<Arc<UiPackage>>,
    error: Option<UiPackageError>,
}

impl UiPackageState {
    /// Returns a state with no package.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Loads the package from a folder, when one is given.
    pub fn from_directory(directory: Option<&Path>) -> Self {
        match directory {
            Some(directory) => match UiPackage::load(directory) {
                Ok(package) => Self::with_package(package),
                Err(error) => Self::failed(error),
            },
            None => Self::empty(),
        }
    }

    /// Returns a state that serves one loaded package.
    pub fn with_package(package: UiPackage) -> Self {
        Self {
            package: Some(Arc::new(package)),
            error: None,
        }
    }

    /// Returns a state that answers every UI request with one failure.
    pub fn failed(error: UiPackageError) -> Self {
        Self {
            package: None,
            error: Some(error),
        }
    }

    /// Returns the manifest, when a package is loaded.
    pub fn manifest(&self) -> Option<&UiManifest> {
        self.package.as_ref().map(|package| package.manifest())
    }

    /// Returns the loaded package.
    pub fn package(&self) -> Option<&Arc<UiPackage>> {
        self.package.as_ref()
    }

    /// Returns the saved failure, when the package could not load.
    pub fn error(&self) -> Option<&UiPackageError> {
        self.error.as_ref()
    }
}

impl fmt::Debug for UiPackageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiPackageState")
            .field("loaded", &self.package.is_some())
            .field("error", &self.error)
            .finish()
    }
}

/// Returns the hex part of a `blake3:<hex>` build id.
pub fn build_hash(build_id: &str) -> Option<&str> {
    let hash = build_id.strip_prefix("blake3:")?;
    if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(hash)
    } else {
        None
    }
}

fn validate_manifest(manifest: &UiManifest) -> Result<(), UiPackageError> {
    if manifest.schema != UI_SCHEMA {
        return Err(UiPackageError::Invalid(format!(
            "the manifest schema {} is not supported",
            manifest.schema
        )));
    }
    if build_hash(&manifest.build_id).is_none() {
        return Err(UiPackageError::Invalid(
            "the build id is not valid".to_owned(),
        ));
    }
    Ok(())
}

fn validate_entries(archive: &[u8], manifest: &UiManifest) -> Result<(), UiPackageError> {
    let mut zip = ZipArchive::new(Cursor::new(archive))
        .map_err(|error| UiPackageError::Invalid(format!("the archive: {error}")))?;
    if zip.len() != manifest.files as usize {
        return Err(UiPackageError::Invalid(
            "the archive entry count does not match the manifest".to_owned(),
        ));
    }
    if zip.len() > MAX_UI_ENTRIES {
        return Err(UiPackageError::TooLarge);
    }

    let mut names = HashSet::with_capacity(zip.len());
    let mut unpacked = 0u64;
    let mut has_index = false;
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|error| UiPackageError::Invalid(format!("the archive: {error}")))?;
        let name = entry
            .name()
            .map_err(|error| UiPackageError::Invalid(format!("an entry name: {error}")))?
            .into_owned();
        check_entry_name(&name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(UiPackageError::Invalid(
                "the archive has duplicate entry names".to_owned(),
            ));
        }
        if entry.is_symlink() {
            return Err(UiPackageError::Invalid(
                "the archive holds a symbolic link".to_owned(),
            ));
        }
        if entry.size() > MAX_UI_ENTRY_BYTES {
            return Err(UiPackageError::TooLarge);
        }
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > MAX_UI_UNPACKED_BYTES {
            return Err(UiPackageError::TooLarge);
        }
        if name == "index.html" {
            has_index = true;
        }
    }

    if !has_index {
        return Err(UiPackageError::Invalid(
            "the archive has no index.html".to_owned(),
        ));
    }
    Ok(())
}

/// Checks one archive entry name.
///
/// A name must be a relative path with normal components only. An absolute
/// path, a parent component, a backslash and a NUL byte are refused.
pub fn check_entry_name(name: &str) -> Result<(), UiPackageError> {
    if name.is_empty() || name.len() > MAX_UI_NAME_BYTES {
        return Err(UiPackageError::Invalid(
            "an entry name is empty or too long".to_owned(),
        ));
    }
    if name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(UiPackageError::Invalid(
            "an entry name is not a relative path".to_owned(),
        ));
    }
    let path = Path::new(name);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(UiPackageError::Invalid(
            "an entry name leaves the package folder".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;

    fn temp_folder(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "elesdee-core-ui-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&path).expect("create temporary folder");
        path
    }

    fn sample_zip() -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("index.html", options).expect("index");
            writer.write_all(b"<!doctype html>").expect("write index");
            writer.start_file("assets/app.js", options).expect("asset");
            writer.write_all(b"console.log('hi')").expect("write asset");
            writer.finish().expect("finish zip");
        }
        output.into_inner()
    }

    fn write_package(directory: &Path, zip: &[u8], files: u32) -> UiManifest {
        let manifest = UiManifest {
            schema: UI_SCHEMA,
            build_id: format!("blake3:{}", blake3::hash(zip).to_hex()),
            app_version: "0.1.0".to_owned(),
            protocol_version: 1,
            min_bridge_version: 1,
            bytes: zip.len() as u64,
            files,
        };
        std::fs::write(
            directory.join(UI_MANIFEST_FILE),
            serde_json::to_vec(&manifest).expect("manifest json"),
        )
        .expect("write manifest");
        std::fs::write(directory.join(UI_ARCHIVE_FILE), zip).expect("write archive");
        manifest
    }

    #[test]
    fn a_valid_package_loads_and_keeps_its_archive() {
        let folder = temp_folder("valid");
        let zip = sample_zip();
        let manifest = write_package(&folder, &zip, 2);

        let package = UiPackage::load(&folder).expect("load package");
        assert_eq!(package.manifest(), &manifest);
        assert_eq!(package.archive(), zip.as_slice());

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_missing_package_is_reported() {
        let folder = temp_folder("missing");
        assert!(matches!(
            UiPackage::load(&folder),
            Err(UiPackageError::Missing)
        ));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_changed_archive_is_rejected() {
        let folder = temp_folder("hash");
        let zip = sample_zip();
        write_package(&folder, &zip, 2);
        let mut changed = zip.clone();
        changed.push(0);
        std::fs::write(folder.join(UI_ARCHIVE_FILE), &changed).expect("rewrite archive");

        let error = UiPackage::load(&folder).expect_err("hash mismatch");
        assert!(matches!(error, UiPackageError::Invalid(_)));

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn an_oversized_archive_is_rejected_before_it_is_read() {
        let folder = temp_folder("large");
        let zip = sample_zip();
        write_package(&folder, &zip, 2);
        let archive = folder.join(UI_ARCHIVE_FILE);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&archive)
            .expect("open archive");
        file.set_len(MAX_UI_PACKAGE_BYTES as u64 + 1)
            .expect("size archive");

        assert!(matches!(
            UiPackage::load(&folder),
            Err(UiPackageError::TooLarge)
        ));

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn an_entry_that_leaves_the_package_is_rejected() {
        for name in ["../evil.js", "/etc/passwd", "assets\\evil.js", "a//b"] {
            assert!(
                check_entry_name(name).is_err(),
                "the name was accepted: {name}"
            );
        }
        assert!(check_entry_name("assets/app.js").is_ok());
        assert!(check_entry_name("index.html").is_ok());
    }

    #[test]
    fn a_build_id_must_carry_a_blake3_hash() {
        let hash = blake3::hash(b"package").to_hex().to_string();
        assert_eq!(build_hash(&format!("blake3:{hash}")), Some(hash.as_str()));
        assert_eq!(build_hash(&format!("sha256:{hash}")), None);
        assert_eq!(build_hash("blake3:abc"), None);
    }

    #[test]
    fn a_package_without_an_index_is_rejected() {
        let folder = temp_folder("no-index");
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("app.js", options).expect("asset");
            writer.write_all(b"console.log('hi')").expect("write asset");
            writer.finish().expect("finish zip");
        }
        let zip = output.into_inner();
        write_package(&folder, &zip, 1);

        assert!(matches!(
            UiPackage::load(&folder),
            Err(UiPackageError::Invalid(_))
        ));

        let _ = std::fs::remove_dir_all(folder);
    }
}
