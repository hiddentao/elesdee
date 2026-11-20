use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

/// The largest text file the API reads or writes, in bytes.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// The most entries one folder listing returns.
pub const MAX_TREE_ENTRIES: usize = 2000;
const SNIFF_BYTES: usize = 8192;

#[derive(Debug)]
pub enum FileError {
    InvalidPath,
    OutsideRoot,
    NotFound,
    NotAFile,
    NotADirectory,
    TooLarge(u64),
    Binary,
    Io(io::Error),
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            FileError::NotFound
        } else {
            FileError::Io(error)
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub directory: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileList {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub size: u64,
}

/// Lists one folder below the session root. An empty path lists the root.
pub fn list(root: &Path, relative: &str) -> Result<FileList, FileError> {
    let relative = relative.trim();
    let target = if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(valid_relative(relative)?)
    };

    let resolved = fs::canonicalize(&target)?;
    if !resolved.starts_with(root) {
        return Err(FileError::OutsideRoot);
    }
    if !fs::metadata(&resolved)?.is_dir() {
        return Err(FileError::NotADirectory);
    }

    let mut entries = Vec::new();
    let mut truncated = false;
    for item in fs::read_dir(&resolved)? {
        let Ok(item) = item else {
            continue;
        };
        let Ok(file_type) = item.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        if name.is_empty() || (file_type.is_dir() && name == ".git") {
            continue;
        }
        if entries.len() >= MAX_TREE_ENTRIES {
            truncated = true;
            break;
        }
        let path = item
            .path()
            .strip_prefix(root)
            .map(path_string)
            .unwrap_or_else(|_| name.clone());
        entries.push(FileEntry {
            name,
            path,
            directory: file_type.is_dir(),
        });
    }

    entries.sort_by(|left, right| {
        right
            .directory
            .cmp(&left.directory)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(FileList {
        path: relative.to_owned(),
        entries,
        truncated,
    })
}

/// Reads a UTF-8 text file below the session root.
pub fn read(root: &Path, relative: &str) -> Result<FileContent, FileError> {
    let target = root.join(valid_relative(relative)?);
    let resolved = fs::canonicalize(&target)?;
    if !resolved.starts_with(root) {
        return Err(FileError::OutsideRoot);
    }

    let metadata = fs::metadata(&resolved)?;
    if !metadata.is_file() {
        return Err(FileError::NotAFile);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(FileError::TooLarge(metadata.len()));
    }

    let bytes = fs::read(&resolved)?;
    if is_binary(&bytes) {
        return Err(FileError::Binary);
    }
    let content = String::from_utf8(bytes).map_err(|_| FileError::Binary)?;
    Ok(FileContent {
        path: relative.to_owned(),
        size: content.len() as u64,
        content,
    })
}

/// Writes a UTF-8 text file below the session root.
pub fn write(root: &Path, relative: &str, content: &str) -> Result<FileContent, FileError> {
    let bytes = content.as_bytes();
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(FileError::TooLarge(bytes.len() as u64));
    }
    if is_binary(bytes) {
        return Err(FileError::Binary);
    }

    let target = root.join(valid_relative(relative)?);
    let parent = target.parent().ok_or(FileError::InvalidPath)?;
    let resolved_parent = fs::canonicalize(parent)?;
    if !resolved_parent.starts_with(root) {
        return Err(FileError::OutsideRoot);
    }

    if fs::symlink_metadata(&target).is_ok() {
        let resolved = fs::canonicalize(&target)?;
        if !resolved.starts_with(root) {
            return Err(FileError::OutsideRoot);
        }
        if !fs::metadata(&resolved)?.is_file() {
            return Err(FileError::NotAFile);
        }
    }

    fs::write(&target, bytes)?;
    Ok(FileContent {
        path: relative.to_owned(),
        size: bytes.len() as u64,
        content: content.to_owned(),
    })
}

fn valid_relative(relative: &str) -> Result<PathBuf, FileError> {
    let trimmed = relative.trim();
    if trimmed.is_empty() || trimmed.len() > 4096 {
        return Err(FileError::InvalidPath);
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(FileError::InvalidPath);
    }

    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => clean.push(part),
            _ => return Err(FileError::InvalidPath),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(FileError::InvalidPath);
    }
    Ok(clean)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(SNIFF_BYTES).any(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "elesdee-files-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }

    #[test]
    fn reads_and_writes_inside_the_root() {
        let root = temp("roundtrip");
        fs::write(root.join("note.txt"), "hello\n").unwrap();

        let file = read(&root, "note.txt").expect("read file");
        assert_eq!(file.content, "hello\n");
        assert_eq!(file.size, 6);

        let written = write(&root, "note.txt", "changed\n").expect("write file");
        assert_eq!(written.content, "changed\n");
        assert_eq!(
            fs::read_to_string(root.join("note.txt")).unwrap(),
            "changed\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let root = temp("traversal");
        for path in ["../outside.txt", "a/../../outside.txt", "/etc/passwd", ""] {
            assert!(
                matches!(read(&root, path), Err(FileError::InvalidPath)),
                "{path} must be rejected"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_a_directory_where_a_file_is_expected() {
        let root = temp("directory");
        fs::create_dir_all(root.join("src")).unwrap();
        assert!(matches!(read(&root, "src"), Err(FileError::NotAFile)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_binary_content() {
        let root = temp("binary");
        fs::write(root.join("blob.bin"), [0_u8, 1, 2]).unwrap();
        assert!(matches!(read(&root, "blob.bin"), Err(FileError::Binary)));
        assert!(matches!(
            write(&root, "blob.bin", "a\0b"),
            Err(FileError::Binary)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_that_escapes_the_root() {
        let root = temp("symlink");
        let outside = temp("symlink-outside");
        fs::write(outside.join("secret.txt"), "secret\n").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();

        assert!(matches!(
            read(&root, "link.txt"),
            Err(FileError::OutsideRoot)
        ));
        assert!(matches!(
            write(&root, "link.txt", "overwritten\n"),
            Err(FileError::OutsideRoot)
        ));
        assert_eq!(
            fs::read_to_string(outside.join("secret.txt")).unwrap(),
            "secret\n"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn lists_folders_first_and_skips_symlinks() {
        let root = temp("list");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::write(root.join("a.txt"), "a").unwrap();

        let root_list = list(&root, "").expect("list root");
        let names = root_list
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["src", "a.txt", "b.txt"]);
        assert!(root_list.entries[0].directory);

        let nested = list(&root, "src").expect("list src");
        assert!(nested.entries.is_empty());
        assert_eq!(nested.path, "src");
        let _ = fs::remove_dir_all(root);
    }
}
