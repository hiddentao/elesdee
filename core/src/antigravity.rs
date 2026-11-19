use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::private_fs;

pub const PROVIDER_ID: &str = "google";
pub const DEFAULT_MODEL: &str = "gemini-3.8-flash-medium";
const HOMES_DIR: &str = "homes";
const SETTINGS: &[u8] = br#"{"modelProvider":"gemini"}"#;

pub fn home_path(data_dir: &Path, session_id: &str) -> PathBuf {
    data_dir.join(HOMES_DIR).join(session_id)
}

pub fn prepare_home(data_dir: &Path, session_id: &str) -> io::Result<PathBuf> {
    let home = home_path(data_dir, session_id);
    let prepared = (|| {
        let settings_dir = home.join(".gemini").join("antigravity-cli");
        for directory in [
            data_dir.join(HOMES_DIR),
            home.clone(),
            home.join(".gemini"),
            settings_dir.clone(),
        ] {
            private_fs::create_private_dir_all(&directory)?;
        }
        private_fs::write_private_file(&settings_dir.join("settings.json"), SETTINGS)
    })();

    if let Err(error) = prepared {
        let _ = fs::remove_dir_all(&home);
        return Err(error);
    }
    Ok(home)
}

pub fn environment(home: &Path, key: &str) -> Vec<(String, String)> {
    vec![
        ("HOME".to_owned(), home.to_string_lossy().into_owned()),
        ("GEMINI_API_KEY".to_owned(), key.to_owned()),
    ]
}

pub fn remove_home(data_dir: &Path, session_id: &str) {
    let _ = fs::remove_dir_all(home_path(data_dir, session_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("elesdee-antigravity-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    #[test]
    fn prepares_a_private_home_and_settings() {
        let data_dir = temporary_directory();
        let home = prepare_home(&data_dir, "session-id").expect("prepare home");
        assert_eq!(home, data_dir.join("homes").join("session-id"));
        let settings = home.join(".gemini/antigravity-cli/settings.json");
        assert_eq!(
            fs::read(&settings).expect("read settings"),
            br#"{"modelProvider":"gemini"}"#
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = fs::metadata(&settings)
                .expect("settings metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
            let directory_mode = fs::metadata(&home)
                .expect("home metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(directory_mode, 0o700);
        }

        let variables = environment(&home, "secret-key");
        assert!(variables.contains(&("HOME".to_owned(), home.to_string_lossy().into_owned())));
        assert!(variables.contains(&("GEMINI_API_KEY".to_owned(), "secret-key".to_owned())));

        remove_home(&data_dir, "session-id");
        assert!(!home.exists());
        fs::remove_dir_all(&data_dir).expect("remove test directory");
    }
}
