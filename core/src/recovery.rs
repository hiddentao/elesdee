use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::api::{
    configure_session_tools, install_claude_settings, secret_environment_values, session_command,
    start_codex_runtime, start_opencode_poller,
};
use crate::clients;
use crate::providers;
use crate::sessions::{self, SessionEntry};
use crate::state::{
    Account, AppState, Auth, Client, Session, SessionEvent, SessionKind, SessionState, WorkState,
};

const RESUMED_PREVIEW: &str = "Resumed.";
const LAST_CONVERSATION_PREVIEW: &str = "Resumed the most recent conversation.";

pub(crate) fn recover(state: &AppState) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let Some(data_dir) = state.data_dir().map(Path::to_path_buf) else {
        return;
    };

    let mut changed = false;
    for entry in state.sessions() {
        let session = entry.session();
        if session.status != SessionState::Active || session.work_state != Some(WorkState::Paused) {
            continue;
        }

        match restart(state, &entry, &session, &data_dir) {
            Ok(updated) => {
                state.emit(SessionEvent::updated(updated));
                changed = true;
            }
            Err(reason) => {
                entry.remove_session_files();
                let paused = entry.pause(&reason);
                state.emit(SessionEvent::updated(paused));
                changed = true;
            }
        }
    }
    if changed {
        state.save();
    }
}

fn restart(
    state: &AppState,
    entry: &Arc<SessionEntry>,
    session: &Session,
    data_dir: &Path,
) -> Result<Session, String> {
    let folder = PathBuf::from(&session.folder);
    if !folder.is_dir() {
        return Err("Paused. The session folder is missing.".to_owned());
    }

    let mut command;
    let mut environment = Vec::new();
    let mut opencode_base = None;
    let preview;

    match session.kind {
        SessionKind::Terminal => {
            command = session_command(state, SessionKind::Terminal, None)
                .map_err(|_| "Paused. The terminal could not be restarted.".to_owned())?;
            preview = RESUMED_PREVIEW;
        }
        SessionKind::Agent => {
            let client = session
                .client
                .ok_or_else(|| "Paused. The session client is missing.".to_owned())?;
            if !client_available(state, client) {
                return Err(format!("Paused. {client} is not installed."));
            }
            let account = session_account(state, session, client)?;
            environment = account_environment(state, &account)?;
            command = session_command(state, SessionKind::Agent, Some(client))
                .map_err(|_| format!("Paused. {client} could not be restarted."))?;
            for argument in sessions::resume_arguments(client, session.agent_session_id.as_deref())
            {
                command.arg(argument);
            }

            if client == Client::ClaudeCode {
                let path = install_claude_settings(state, entry, &session.id)
                    .map_err(|_| "Paused. The session hooks could not be prepared.".to_owned())?;
                if state.command_override().is_none() {
                    crate::claude::add_settings_argument(&mut command, &path);
                }
            }

            if client == Client::Antigravity {
                let key = account_key(state, &account.id)?;
                let home = crate::antigravity::prepare_home(
                    data_dir,
                    &session.id,
                    session.model.as_deref(),
                    &session.config,
                )
                .map_err(|error| {
                    tracing::error!(
                        session = %session.id,
                        %error,
                        "could not prepare the Antigravity home"
                    );
                    "Paused. The session home could not be prepared.".to_owned()
                })?;
                entry.set_home(home.clone());
                environment = crate::antigravity::environment(&home, &key);
            }

            for item in &session.env_overrides {
                environment.push((item.name.clone(), item.value.clone()));
            }

            configure_session_tools(
                state,
                entry,
                &session.id,
                client,
                &mut command,
                &mut environment,
            )
            .map_err(|_| "Paused. The session tools could not be prepared.".to_owned())?;

            if client == Client::Codex {
                start_codex_runtime(state, entry, &mut command)
                    .map_err(|_| "Paused. The Codex app server could not start.".to_owned())?;
            }
            if client == Client::OpenCode2
                && let Some(port) = crate::opencode::free_port()
            {
                crate::opencode::launch_args(&mut command, port);
                let base_url = format!("http://127.0.0.1:{port}");
                entry.set_opencode_runtime(crate::sessions::OpenCodeRuntime {
                    base_url: base_url.clone(),
                    port,
                });
                opencode_base = Some(base_url);
            }

            preview = if session.agent_session_id.is_some() {
                RESUMED_PREVIEW
            } else {
                LAST_CONVERSATION_PREVIEW
            };
        }
    }

    entry.set_redactions(secret_environment_values(&environment));
    let updated = entry.begin_recovery(preview);
    let size = entry.stored_size();
    sessions::spawn(
        entry,
        command,
        &folder,
        size,
        state.stall_writer_for_tests(),
        state.event_sender(),
        &environment,
    )
    .map_err(|error| {
        tracing::warn!(session = %session.id, %error, "could not restart a session");
        "Paused. The session could not be restarted.".to_owned()
    })?;

    if let Some(base_url) = opencode_base {
        start_opencode_poller(state.clone(), Arc::clone(entry), base_url);
    }

    Ok(updated)
}

fn session_account(state: &AppState, session: &Session, client: Client) -> Result<Account, String> {
    let account_id = session
        .account_id
        .as_deref()
        .ok_or_else(|| "Paused. The account is no longer available.".to_owned())?;
    let account = state
        .account(account_id)
        .ok_or_else(|| "Paused. The account is no longer available.".to_owned())?;
    if account.client != client {
        return Err("Paused. The account is no longer available.".to_owned());
    }
    Ok(account)
}

fn client_available(state: &AppState, client: Client) -> bool {
    if state.command_override().is_some() {
        return true;
    }
    if let Some(binary) = state.agent_bin() {
        return clients::is_executable(binary);
    }
    clients::find_client(client).is_some()
}

fn account_environment(
    state: &AppState,
    account: &Account,
) -> Result<Vec<(String, String)>, String> {
    let key = if account.auth == Auth::Key && account.provider != crate::antigravity::PROVIDER_ID {
        Some(account_key(state, &account.id)?)
    } else {
        None
    };
    Ok(providers::environment(account, key.as_deref()))
}

fn account_key(state: &AppState, account_id: &str) -> Result<String, String> {
    let store = state.secret_store();
    match store.get_key(account_id) {
        Ok(Some(key)) if !key.is_empty() => Ok(key),
        Ok(_) => Err("Paused. The account key is not available.".to_owned()),
        Err(error) => {
            tracing::error!(%account_id, %error, "could not read account key");
            Err("Paused. The account key could not be read.".to_owned())
        }
    }
}
