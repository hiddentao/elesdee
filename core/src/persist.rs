use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::history::HistoryItem;
use crate::private_fs;
use crate::seed;
use crate::state::{
    AiConfig, AppSettings, LegacyAccount, LegacyPreset, PairedDevice, Project, Provider, Question,
    Session, SessionRequest,
};

const STATE_VERSION: u32 = 2;
const LEGACY_STATE_VERSION: u32 = 1;
const STATE_FILE: &str = "state.json";
const BAD_FILE: &str = "state.bad.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedState {
    pub version: u32,
    pub projects: Vec<Project>,
    #[serde(default)]
    pub providers: Option<Vec<Provider>>,
    #[serde(default)]
    pub configs: Option<Vec<AiConfig>>,
    #[serde(default, skip_serializing)]
    pub accounts: Option<Vec<LegacyAccount>>,
    #[serde(default, skip_serializing)]
    pub presets: Option<Vec<LegacyPreset>>,
    #[serde(default)]
    pub questions: Option<Vec<Question>>,
    #[serde(default)]
    pub session_requests: Option<Vec<SessionRequest>>,
    #[serde(default)]
    pub paired_devices: Vec<PairedDevice>,
    #[serde(default)]
    pub settings: AppSettings,
    pub sessions: Vec<PersistedSession>,
}

impl PersistedState {
    pub fn new(
        projects: Vec<Project>,
        providers: Option<Vec<Provider>>,
        configs: Option<Vec<AiConfig>>,
        questions: Option<Vec<Question>>,
        session_requests: Option<Vec<SessionRequest>>,
        paired_devices: Vec<PairedDevice>,
        sessions: Vec<PersistedSession>,
    ) -> Self {
        Self {
            version: STATE_VERSION,
            projects,
            providers,
            configs,
            accounts: None,
            presets: None,
            questions,
            session_requests,
            paired_devices,
            settings: AppSettings::default(),
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryItem>,
    #[serde(default)]
    pub next_history_seq: u64,
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
    Loaded(Box<PersistedState>),
    Corrupt,
}

pub(crate) struct StateStore {
    directory: PathBuf,
    state_path: PathBuf,
}

pub(crate) fn data_directory_from_environment() -> Option<PathBuf> {
    env::var_os("ELESDEE_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| seed::home_dir().map(|home| home.join(".elesdee")))
}

impl StateStore {
    pub fn from_environment() -> Option<Self> {
        let directory = data_directory_from_environment()?;

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

        if state.version != STATE_VERSION && state.version != LEGACY_STATE_VERSION {
            return self.corrupt(&format!("unsupported state version {}", state.version));
        }

        LoadResult::Loaded(Box::new(state))
    }

    pub fn save(&self, state: &PersistedState) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        private_fs::write_private_file_atomic(&self.state_path, &bytes)
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
#[cfg(test)]
mod tests {
    use super::*;

    fn stored_state(agent_session_id: Option<&str>) -> serde_json::Value {
        let mut session = serde_json::json!({
            "id": "session-1",
            "name": "Session",
            "short": "session",
            "projectId": "project-1",
            "kind": "agent",
            "status": "active",
            "workState": "working",
            "client": "Claude Code",
            "accountId": "max",
            "branch": "elesdee/project/session",
            "folder": "/tmp/session",
            "preview": "Working.",
            "waitingFor": null,
            "startedAt": "2026-01-01T00:00:00Z",
            "shortcuts": []
        });
        if let Some(id) = agent_session_id {
            session["agentSessionId"] = serde_json::Value::String(id.to_owned());
        }
        serde_json::json!({
            "version": STATE_VERSION,
            "projects": [],
            "sessions": [{
                "session": session,
                "cols": 120,
                "rows": 32,
                "output": [],
                "nextSeq": 1
            }]
        })
    }

    #[test]
    fn persisted_session_keeps_the_agent_session_id() {
        let state: PersistedState =
            serde_json::from_value(stored_state(Some("conversation-1"))).expect("parse state");
        assert_eq!(
            state.sessions[0].session.agent_session_id.as_deref(),
            Some("conversation-1")
        );

        let value = serde_json::to_value(&state).expect("serialize state");
        assert_eq!(
            value["sessions"][0]["session"]["agentSessionId"],
            "conversation-1"
        );
    }

    #[test]
    fn older_sessions_without_an_agent_session_id_still_load() {
        let state: PersistedState =
            serde_json::from_value(stored_state(None)).expect("parse state");
        assert!(state.sessions[0].session.agent_session_id.is_none());
    }
}
