use std::env;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::private_fs;
use crate::seed;
use crate::state::{Account, Project, Session};

const STATE_VERSION: u32 = 1;
const STATE_FILE: &str = "state.json";
const TEMP_FILE: &str = "state.json.tmp";
const BAD_FILE: &str = "state.bad.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedState {
    pub version: u32,
    pub projects: Vec<Project>,
    #[serde(default)]
    pub accounts: Option<Vec<Account>>,
    pub sessions: Vec<PersistedSession>,
}

impl PersistedState {
    pub fn new(
        projects: Vec<Project>,
        accounts: Option<Vec<Account>>,
        sessions: Vec<PersistedSession>,
    ) -> Self {
        Self {
            version: STATE_VERSION,
            projects,
            accounts,
            sessions,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedSession {
    pub session: Session,
    pub cols: u16,
    pub rows: u16,
    pub output: Vec<PersistedOutput>,
    pub next_seq: u64,
    #[serde(default)]
    pub repo_root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedOutput {
    pub seq: u64,
    pub data: String,
}

pub(crate) enum LoadResult {
    Missing,
    Loaded(PersistedState),
    Corrupt,
}

pub(crate) struct StateStore {
    directory: PathBuf,
    state_path: PathBuf,
}

impl StateStore {
    pub fn from_environment() -> Option<Self> {
        let directory = env::var_os("ELESDEE_DATA_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| seed::home_dir().map(|home| home.join(".elesdee")))?;

        match Self::new(directory) {
            Ok(store) => Some(store),
            Err(error) => {
                tracing::error!(%error, "could not prepare the data directory");
                None
            }
        }
    }

    pub fn new(directory: PathBuf) -> io::Result<Self> {
        let directory = if directory.is_absolute() {
            directory
        } else {
            env::current_dir()?.join(directory)
        };
        private_fs::create_private_dir_all(&directory)?;
        let state_path = directory.join(STATE_FILE);
        Ok(Self {
            directory,
            state_path,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn load(&self) -> LoadResult {
        let bytes = match fs::read(&self.state_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return LoadResult::Missing,
            Err(error) => return self.corrupt(&format!("could not read state: {error}")),
        };

        let state = match serde_json::from_slice::<PersistedState>(&bytes) {
            Ok(state) => state,
            Err(error) => return self.corrupt(&format!("could not parse state: {error}")),
        };

        if state.version != STATE_VERSION {
            return self.corrupt(&format!("unsupported state version {}", state.version));
        }

        LoadResult::Loaded(state)
    }

    pub fn save(&self, state: &PersistedState) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        let temporary = self.directory.join(TEMP_FILE);

        let write_result = (|| {
            private_fs::write_private_file(&temporary, &bytes)?;
            fs::rename(&temporary, &self.state_path)?;
            sync_directory(&self.directory);
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }

    fn corrupt(&self, message: &str) -> LoadResult {
        tracing::error!(
            path = %self.state_path.display(),
            error = message,
            "state file is corrupt; starting from seeds"
        );
        let bad_path = self.directory.join(BAD_FILE);
        if bad_path.exists() {
            let _ = fs::remove_file(&bad_path);
        }
        if let Err(error) = fs::rename(&self.state_path, &bad_path) {
            tracing::error!(
                path = %self.state_path.display(),
                %error,
                "could not move the corrupt state file aside"
            );
        }
        LoadResult::Corrupt
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) {
    if let Ok(directory) = File::open(path) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) {}
