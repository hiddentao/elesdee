use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::files;
use crate::private_fs;

/// The largest file a session may share, in bytes.
pub const MAX_SHARED_BYTES: u64 = 10 * 1024 * 1024;
const META_EXTENSION: &str = "json";
const SECRET_DIRECTORIES: [&str; 6] = [".ssh", ".gnupg", ".aws", ".azure", ".kube", ".git"];
const SECRET_NAMES: [&str; 15] = [
    ".env",
    ".npmrc",
    ".netrc",
    ".git-credentials",
    "credentials",
    "credentials.json",
    "auth.json",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "secrets.json",
    "secrets.toml",
    "secrets.yaml",
    "secrets.yml",
];
const SECRET_SUFFIXES: [&str; 8] = [
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".keystore",
    ".kdbx",
    ".jks",
    ".ppk",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedFile {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub name: String,
    pub path: String,
    pub caption: Option<String>,
    pub size: u64,
    pub content_type: String,
    pub image: bool,
    pub created_at: String,
}

#[derive(Debug)]
pub enum ShareError {
    InvalidPath,
    OutsideFolder,
    NotFound,
    NotAFile,
    TooLarge(u64),
    Linked,
    Secret,
    Unavailable,
    Io(io::Error),
}

impl From<io::Error> for ShareError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => ShareError::NotFound,
            io::ErrorKind::NotADirectory => ShareError::NotAFile,
            io::ErrorKind::IsADirectory => ShareError::NotAFile,
            io::ErrorKind::InvalidInput => ShareError::InvalidPath,
            _ => ShareError::Io(error),
        }
    }
}

/// Copies a file from the session folder into the shared store.
pub fn share(
    root: &Path,
    data_dir: &Path,
    session_id: &str,
    session_name: &str,
    relative: &str,
    caption: Option<String>,
    secrets: &[String],
) -> Result<SharedFile, ShareError> {
    let relative = relative.trim();
    let clean = valid_relative(relative)?;
    if has_secret_component(&clean) {
        return Err(ShareError::Secret);
    }
    let name = clean
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ShareError::InvalidPath)?
        .to_owned();
    if is_secret_name(&name) {
        return Err(ShareError::Secret);
    }

    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(ShareError::Unavailable);
    }
    let target = root.join(&clean);
    let resolved = target.canonicalize()?;
    if !resolved.starts_with(&root) {
        return Err(ShareError::OutsideFolder);
    }

    let file = files::open_readonly(&resolved)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(ShareError::NotAFile);
    }
    if metadata.len() > MAX_SHARED_BYTES {
        return Err(ShareError::TooLarge(metadata.len()));
    }
    files::reject_hard_link(&metadata).map_err(|_| ShareError::Linked)?;
    let bytes = fs::read(&resolved)?;
    if bytes.len() as u64 > MAX_SHARED_BYTES {
        return Err(ShareError::TooLarge(bytes.len() as u64));
    }
    if looks_like_secret(&bytes, secrets) {
        return Err(ShareError::Secret);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let directory = shared_dir(data_dir, session_id)?;
    let (content_type, image) = classify(&name, &bytes);
    let file = SharedFile {
        id,
        session_id: session_id.to_owned(),
        session_name: session_name.to_owned(),
        name,
        path: path_string(&clean),
        caption: caption.and_then(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.chars().take(200).collect())
        }),
        size: bytes.len() as u64,
        content_type,
        image,
        created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    store_record(&directory, file, &bytes)
}

/// Stores bytes that the app produced itself, such as a browser screenshot.
pub fn share_bytes(
    data_dir: &Path,
    session_id: &str,
    session_name: &str,
    name: &str,
    caption: Option<String>,
    bytes: &[u8],
) -> Result<SharedFile, ShareError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || name.contains(['/', '\\']) {
        return Err(ShareError::InvalidPath);
    }
    if bytes.len() as u64 > MAX_SHARED_BYTES {
        return Err(ShareError::TooLarge(bytes.len() as u64));
    }
    let directory = shared_dir(data_dir, session_id)?;
    let (content_type, image) = classify(name, bytes);
    let file = SharedFile {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_owned(),
        session_name: session_name.to_owned(),
        name: name.to_owned(),
        path: name.to_owned(),
        caption: caption.and_then(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.chars().take(200).collect())
        }),
        size: bytes.len() as u64,
        content_type,
        image,
        created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    store_record(&directory, file, bytes)
}

fn store_record(
    directory: &Path,
    file: SharedFile,
    bytes: &[u8],
) -> Result<SharedFile, ShareError> {
    private_fs::create_private_dir_all(directory)?;
    let stored_name = stored_name(&file.id, &file.name);
    private_fs::write_private_file_atomic(&directory.join(&stored_name), bytes)?;
    let encoded = serde_json::to_vec(&file).map_err(|error| {
        tracing::error!(%error, "could not encode the shared file record");
        ShareError::Unavailable
    })?;
    if let Err(error) =
        private_fs::write_private_file_atomic(&directory.join(meta_name(&file.id)), &encoded)
    {
        let _ = fs::remove_file(directory.join(&stored_name));
        return Err(error.into());
    }
    Ok(file)
}

/// Lists the files shared from one session, newest first.
pub fn list(data_dir: &Path, session_id: &str) -> Result<Vec<SharedFile>, ShareError> {
    let directory = shared_dir(data_dir, session_id)?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !name.starts_with("meta-")
            || path.extension().and_then(|value| value.to_str()) != Some(META_EXTENSION)
        {
            continue;
        }
        match fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SharedFile>(&bytes).ok())
        {
            Some(file) => files.push(file),
            None => tracing::warn!(path = %path.display(), "could not read a shared file record"),
        }
    }
    files.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(files)
}

/// Reads the stored copy of one shared file.
pub fn read(
    data_dir: &Path,
    session_id: &str,
    file_id: &str,
) -> Result<(SharedFile, Vec<u8>), ShareError> {
    if !valid_file_id(file_id) {
        return Err(ShareError::InvalidPath);
    }
    let directory = shared_dir(data_dir, session_id)?;
    let meta_path = directory.join(meta_name(file_id));
    let bytes = match fs::read(&meta_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ShareError::NotFound);
        }
        Err(error) => return Err(error.into()),
    };
    let file: SharedFile = serde_json::from_slice(&bytes).map_err(|_| ShareError::NotFound)?;
    if file.id != file_id || file.session_id != session_id {
        return Err(ShareError::NotFound);
    }
    let directory = directory.canonicalize()?;
    let path = directory.join(stored_name(&file.id, &file.name));
    let resolved = path.canonicalize()?;
    if !resolved.starts_with(&directory) {
        return Err(ShareError::OutsideFolder);
    }
    let handle = files::open_readonly(&resolved)?;
    let metadata = handle.metadata()?;
    if !metadata.is_file() {
        return Err(ShareError::NotAFile);
    }
    if metadata.len() > MAX_SHARED_BYTES {
        return Err(ShareError::TooLarge(metadata.len()));
    }
    Ok((file, fs::read(&resolved)?))
}

fn shared_dir(data_dir: &Path, session_id: &str) -> Result<PathBuf, ShareError> {
    if !valid_session_id(session_id) {
        return Err(ShareError::InvalidPath);
    }
    Ok(data_dir.join("shared").join(session_id))
}

fn meta_name(file_id: &str) -> String {
    format!("meta-{file_id}.{META_EXTENSION}")
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn valid_relative(relative: &str) -> Result<PathBuf, ShareError> {
    files::valid_relative(relative).map_err(|_| ShareError::InvalidPath)
}

fn has_secret_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(part) => {
            let part = part.to_string_lossy().to_ascii_lowercase();
            SECRET_DIRECTORIES.iter().any(|name| part == *name)
        }
        _ => true,
    })
}

fn is_secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SECRET_NAMES.iter().any(|secret| name == *secret)
        || name.starts_with(".env.")
        || SECRET_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

fn looks_like_secret(bytes: &[u8], secrets: &[String]) -> bool {
    if contains(bytes, b"PRIVATE KEY-----") {
        return true;
    }
    secrets
        .iter()
        .any(|secret| secret.len() >= 8 && contains(bytes, secret.as_bytes()))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn classify(name: &str, bytes: &[u8]) -> (String, bool) {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => ("image/png".to_owned(), true),
        "jpg" | "jpeg" => ("image/jpeg".to_owned(), true),
        "gif" => ("image/gif".to_owned(), true),
        "webp" => ("image/webp".to_owned(), true),
        "avif" => ("image/avif".to_owned(), true),
        "bmp" => ("image/bmp".to_owned(), true),
        _ if !files::is_binary(bytes) && std::str::from_utf8(bytes).is_ok() => {
            ("text/plain; charset=utf-8".to_owned(), false)
        }
        _ => ("application/octet-stream".to_owned(), false),
    }
}

fn stored_name(id: &str, name: &str) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(10)
        .collect::<String>()
        .to_ascii_lowercase();
    if extension.is_empty() {
        format!("data-{id}")
    } else {
        format!("data-{id}.{extension}")
    }
}

fn valid_file_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "elesdee-shared-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }

    #[test]
    fn shares_a_file_and_reads_it_back() {
        let root = temp("roundtrip-root");
        let data = temp("roundtrip-data");
        fs::write(root.join("shot.png"), [0x89, b'P', b'N', b'G', 0]).unwrap();

        let file = share(
            &root,
            &data,
            "session-1",
            "Session",
            "shot.png",
            Some("Login".to_owned()),
            &[],
        )
        .expect("share file");
        assert_eq!(file.name, "shot.png");
        assert!(file.image);
        assert_eq!(file.caption.as_deref(), Some("Login"));

        let listed = list(&data, "session-1").expect("list shared files");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, file.id);

        let (read_file, bytes) = read(&data, "session-1", &file.id).expect("read shared file");
        assert_eq!(read_file.id, file.id);
        assert_eq!(bytes, [0x89, b'P', b'N', b'G', 0]);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn rejects_a_path_outside_the_session_folder() {
        let root = temp("outside-root");
        let outside = temp("outside-file");
        let data = temp("outside-data");
        fs::write(outside.join("secret.txt"), "not for sharing\n").unwrap();

        assert!(matches!(
            share(
                &root,
                &data,
                "session-1",
                "Session",
                "../outside.txt",
                None,
                &[]
            ),
            Err(ShareError::InvalidPath)
        ));
        let absolute = outside.join("secret.txt");
        assert!(matches!(
            share(
                &root,
                &data,
                "session-1",
                "Session",
                absolute.to_str().unwrap(),
                None,
                &[]
            ),
            Err(ShareError::InvalidPath)
        ));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
        let _ = fs::remove_dir_all(data);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_that_leaves_the_session_folder() {
        let root = temp("symlink-root");
        let outside = temp("symlink-outside");
        let data = temp("symlink-data");
        fs::write(outside.join("secret.txt"), "not for sharing\n").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();

        assert!(matches!(
            share(&root, &data, "session-1", "Session", "link.txt", None, &[]),
            Err(ShareError::OutsideFolder)
        ));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn refuses_secret_files_and_content() {
        let root = temp("secret-root");
        let data = temp("secret-data");
        fs::write(root.join(".env"), "TOKEN=value\n").unwrap();
        fs::write(root.join("notes.txt"), "key: sk-session-secret\n").unwrap();
        fs::write(
            root.join("key.pem"),
            "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----\n",
        )
        .unwrap();

        assert!(matches!(
            share(&root, &data, "session-1", "Session", ".env", None, &[]),
            Err(ShareError::Secret)
        ));
        assert!(matches!(
            share(&root, &data, "session-1", "Session", "key.pem", None, &[]),
            Err(ShareError::Secret)
        ));
        assert!(matches!(
            share(
                &root,
                &data,
                "session-1",
                "Session",
                "notes.txt",
                None,
                &["sk-session-secret".to_owned()]
            ),
            Err(ShareError::Secret)
        ));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn refuses_a_folder_or_a_missing_file() {
        let root = temp("folder-root");
        let data = temp("folder-data");
        fs::create_dir_all(root.join("src")).unwrap();

        assert!(matches!(
            share(&root, &data, "session-1", "Session", "src", None, &[]),
            Err(ShareError::NotAFile)
        ));
        assert!(matches!(
            share(
                &root,
                &data,
                "session-1",
                "Session",
                "missing.txt",
                None,
                &[]
            ),
            Err(ShareError::NotFound)
        ));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn writes_private_shared_files() {
        let root = temp("mode-root");
        let data = temp("mode-data");
        fs::write(root.join("note.txt"), "hello\n").unwrap();
        let file =
            share(&root, &data, "session-1", "Session", "note.txt", None, &[]).expect("share file");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let directory = data.join("shared").join("session-1");
            let file_mode = fs::metadata(directory.join(stored_name(&file.id, &file.name)))
                .expect("shared file metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
            let directory_mode = fs::metadata(&directory)
                .expect("shared directory metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(directory_mode, 0o700);
        }

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(data);
    }
}
