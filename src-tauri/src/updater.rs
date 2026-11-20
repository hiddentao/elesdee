//! Desktop update checks, downloads and installs.
//!
//! The shell checks this repository's GitHub Releases at launch and every few
//! hours, downloads a newer package in the background and verifies its
//! signature before it offers the install. The UI decides whether a restart is
//! safe; a running session defers the install until the user agrees or every
//! session is closed.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

/// The event the UI listens to for a new update snapshot.
pub const UPDATE_EVENT: &str = "elesdee://update-changed";

const UPDATE_ENDPOINT: &str =
    "https://github.com/hiddentao/elesdee/releases/latest/download/latest.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(8);
const CHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateState {
    Idle,
    Checking,
    Downloading,
    Ready,
    UpToDate,
    Deferred,
    Installing,
    Error,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateError {
    NotConfigured,
    Network,
    Signature,
    CheckFailed,
    DownloadFailed,
    InstallFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub state: UpdateState,
    pub current_version: String,
    pub version: Option<String>,
    pub progress: Option<u8>,
    pub error: Option<UpdateError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateSignal {
    CheckStarted,
    Found,
    NoUpdate,
    CheckFailed,
    Downloaded,
    DownloadFailed,
    InstallRequested {
        sessions_running: bool,
        restart_confirmed: bool,
    },
    SessionsClosed,
    InstallFailed,
}

/// Returns the next state for a signal. A signal that does not fit the current
/// state leaves it unchanged.
pub fn next_state(state: UpdateState, signal: UpdateSignal) -> UpdateState {
    use UpdateSignal as Signal;
    use UpdateState as State;
    match (state, signal) {
        (State::Unavailable, _) => State::Unavailable,
        (State::Idle | State::UpToDate | State::Error, Signal::CheckStarted) => State::Checking,
        (State::Checking, Signal::Found) => State::Downloading,
        (State::Checking, Signal::NoUpdate) => State::UpToDate,
        (State::Checking, Signal::CheckFailed) => State::Error,
        (State::Checking, Signal::DownloadFailed) => State::Error,
        (State::Downloading, Signal::Downloaded) => State::Ready,
        (State::Downloading, Signal::DownloadFailed) => State::Error,
        (
            State::Ready | State::Deferred,
            Signal::InstallRequested {
                sessions_running: false,
                ..
            },
        ) => State::Installing,
        (
            State::Ready | State::Deferred,
            Signal::InstallRequested {
                restart_confirmed: true,
                ..
            },
        ) => State::Installing,
        (State::Ready | State::Deferred, Signal::InstallRequested { .. }) => State::Deferred,
        (State::Deferred, Signal::SessionsClosed) => State::Installing,
        (State::Installing, Signal::InstallFailed) => State::Error,
        _ => state,
    }
}

fn initial_snapshot(current_version: &str, configured: bool) -> UpdateSnapshot {
    UpdateSnapshot {
        state: if configured {
            UpdateState::Idle
        } else {
            UpdateState::Unavailable
        },
        current_version: current_version.to_owned(),
        version: None,
        progress: None,
        error: (!configured).then_some(UpdateError::NotConfigured),
    }
}

/// The public key is put into the binary by the release build. A build without
/// it offers no updates and says so.
fn updater_pubkey() -> Option<&'static str> {
    option_env!("TAURI_UPDATER_PUBKEY").filter(|key| !key.trim().is_empty())
}

struct PendingUpdate {
    update: Update,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Runtime {
    snapshot: Option<UpdateSnapshot>,
    pending: Option<PendingUpdate>,
}

pub struct UpdateManager(Mutex<Runtime>);

fn with_runtime<R>(app: &AppHandle, run: impl FnOnce(&mut Runtime) -> R) -> Option<R> {
    let manager = app.try_state::<UpdateManager>()?;
    let mut guard = match manager.0.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    Some(run(&mut guard))
}

fn current_snapshot(app: &AppHandle) -> UpdateSnapshot {
    with_runtime(app, |runtime| runtime.snapshot.clone())
        .flatten()
        .unwrap_or_else(|| {
            initial_snapshot(
                &app.package_info().version.to_string(),
                updater_pubkey().is_some(),
            )
        })
}

fn emit_snapshot(app: &AppHandle, snapshot: &UpdateSnapshot) {
    let _ = app.emit(UPDATE_EVENT, snapshot);
}

fn emit_current(app: &AppHandle) {
    if let Some(snapshot) = with_runtime(app, |runtime| runtime.snapshot.clone()).flatten() {
        emit_snapshot(app, &snapshot);
    }
}

fn set_state(app: &AppHandle, state: UpdateState, error: Option<UpdateError>) {
    let snapshot = with_runtime(app, |runtime| {
        let snapshot = runtime
            .snapshot
            .get_or_insert_with(|| initial_snapshot(&app.package_info().version.to_string(), true));
        snapshot.state = state;
        snapshot.error = error;
        snapshot.progress = None;
        snapshot.clone()
    });
    if let Some(snapshot) = snapshot {
        emit_snapshot(app, &snapshot);
    }
}

fn update_error(error: &UpdaterError) -> UpdateError {
    match error {
        UpdaterError::Minisign(_)
        | UpdaterError::Base64(_)
        | UpdaterError::SignatureUtf8(_)
        | UpdaterError::SignedVersionMismatch { .. } => UpdateError::Signature,
        UpdaterError::Reqwest(_)
        | UpdaterError::Network(_)
        | UpdaterError::Http(_)
        | UpdaterError::InvalidHeaderValue(_)
        | UpdaterError::InvalidHeaderName(_) => UpdateError::Network,
        _ => UpdateError::DownloadFailed,
    }
}

fn check_error(error: &UpdaterError) -> UpdateError {
    match update_error(error) {
        UpdateError::Network => UpdateError::Network,
        _ => UpdateError::CheckFailed,
    }
}

fn install_error(error: &UpdaterError) -> UpdateError {
    if update_error(error) == UpdateError::Signature {
        UpdateError::Signature
    } else {
        UpdateError::InstallFailed
    }
}

async fn check_and_download(app: AppHandle) {
    let Some(pubkey) = updater_pubkey() else {
        set_state(
            &app,
            UpdateState::Unavailable,
            Some(UpdateError::NotConfigured),
        );
        return;
    };

    let started = with_runtime(&app, |runtime| {
        let Some(snapshot) = runtime.snapshot.as_mut() else {
            return false;
        };
        let next = next_state(snapshot.state, UpdateSignal::CheckStarted);
        if next == snapshot.state {
            return false;
        }
        snapshot.state = next;
        snapshot.version = None;
        snapshot.progress = None;
        snapshot.error = None;
        true
    });
    if started != Some(true) {
        return;
    }
    emit_current(&app);

    let endpoint = match UPDATE_ENDPOINT.parse() {
        Ok(endpoint) => endpoint,
        Err(_) => {
            set_state(&app, UpdateState::Error, Some(UpdateError::CheckFailed));
            return;
        }
    };
    let builder = app
        .updater_builder()
        .pubkey(pubkey)
        .timeout(REQUEST_TIMEOUT);
    let updater = match builder
        .endpoints(vec![endpoint])
        .and_then(|builder| builder.build())
    {
        Ok(updater) => updater,
        Err(error) => {
            eprintln!("elesdee: could not prepare the update check: {error}");
            set_state(&app, UpdateState::Error, Some(check_error(&error)));
            return;
        }
    };

    match updater.check().await {
        Ok(None) => set_state(
            &app,
            next_state(UpdateState::Checking, UpdateSignal::NoUpdate),
            None,
        ),
        Ok(Some(update)) => download_update(app, update).await,
        Err(error) => {
            eprintln!("elesdee: update check failed: {error}");
            set_state(
                &app,
                next_state(UpdateState::Checking, UpdateSignal::CheckFailed),
                Some(check_error(&error)),
            );
        }
    }
}

async fn download_update(app: AppHandle, update: Update) {
    let version = update.version.clone();
    let snapshot = with_runtime(&app, |runtime| {
        let snapshot = runtime.snapshot.as_mut()?;
        snapshot.state = next_state(UpdateState::Checking, UpdateSignal::Found);
        snapshot.version = Some(version.clone());
        snapshot.progress = Some(0);
        snapshot.error = None;
        Some(snapshot.clone())
    });
    if let Some(snapshot) = snapshot.flatten() {
        emit_snapshot(&app, &snapshot);
    }

    let mut downloaded: u64 = 0;
    let mut last_progress: Option<u8> = None;
    let progress_app = app.clone();
    let result = update
        .download(
            move |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                let progress = total
                    .filter(|total| *total > 0)
                    .map(|total| (downloaded.saturating_mul(100) / total).min(99) as u8);
                if progress == last_progress {
                    return;
                }
                last_progress = progress;
                let snapshot = with_runtime(&progress_app, |runtime| {
                    let snapshot = runtime.snapshot.as_mut()?;
                    snapshot.progress = progress;
                    Some(snapshot.clone())
                });
                if let Some(snapshot) = snapshot.flatten() {
                    emit_snapshot(&progress_app, &snapshot);
                }
            },
            || {},
        )
        .await;

    match result {
        Ok(bytes) => {
            let snapshot = with_runtime(&app, |runtime| {
                runtime.pending = Some(PendingUpdate { update, bytes });
                let snapshot = runtime.snapshot.as_mut()?;
                snapshot.state = next_state(UpdateState::Downloading, UpdateSignal::Downloaded);
                snapshot.progress = Some(100);
                snapshot.error = None;
                Some(snapshot.clone())
            });
            if let Some(snapshot) = snapshot.flatten() {
                emit_snapshot(&app, &snapshot);
            }
        }
        Err(error) => {
            eprintln!("elesdee: update download failed: {error}");
            set_state(
                &app,
                next_state(UpdateState::Downloading, UpdateSignal::DownloadFailed),
                Some(update_error(&error)),
            );
        }
    }
}

/// Checks the release feed and downloads a newer signed package.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> UpdateSnapshot {
    check_and_download(app.clone()).await;
    current_snapshot(&app)
}

#[tauri::command]
pub fn get_update_state(app: AppHandle) -> UpdateSnapshot {
    current_snapshot(&app)
}

/// Marks a downloaded update to install when every session is closed.
#[tauri::command]
pub fn defer_update(app: AppHandle) -> UpdateSnapshot {
    let changed = with_runtime(&app, |runtime| {
        let Some(snapshot) = runtime.snapshot.as_mut() else {
            return false;
        };
        if !matches!(snapshot.state, UpdateState::Ready | UpdateState::Deferred) {
            return false;
        }
        snapshot.state = UpdateState::Deferred;
        snapshot.error = None;
        true
    });
    if changed == Some(true) {
        emit_current(&app);
    }
    current_snapshot(&app)
}

/// Stops the core, installs the verified package and restarts the app.
///
/// A running session defers the install unless the user confirmed the restart.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    sessions_running: bool,
    restart_confirmed: bool,
) -> Result<UpdateSnapshot, String> {
    let current = current_snapshot(&app);
    let signal = if current.state == UpdateState::Deferred && !sessions_running {
        UpdateSignal::SessionsClosed
    } else {
        UpdateSignal::InstallRequested {
            sessions_running,
            restart_confirmed,
        }
    };
    match next_state(current.state, signal) {
        UpdateState::Deferred => {
            with_runtime(&app, |runtime| {
                if let Some(snapshot) = runtime.snapshot.as_mut() {
                    snapshot.state = UpdateState::Deferred;
                    snapshot.error = None;
                }
            });
            emit_current(&app);
            return Ok(current_snapshot(&app));
        }
        UpdateState::Installing => {}
        _ => return Ok(current),
    }

    let pending = with_runtime(&app, |runtime| runtime.pending.take()).flatten();
    let Some(pending) = pending else {
        return Ok(current);
    };

    let snapshot = with_runtime(&app, |runtime| {
        let snapshot = runtime.snapshot.as_mut()?;
        snapshot.state = UpdateState::Installing;
        snapshot.progress = None;
        snapshot.error = None;
        Some(snapshot.clone())
    });
    if let Some(snapshot) = snapshot.flatten() {
        emit_snapshot(&app, &snapshot);
    }

    // Save the sessions and stop the service before its bundle is replaced.
    crate::stop_core_for_update(&app);
    match pending.update.install(&pending.bytes) {
        Ok(()) => app.restart(),
        Err(error) => {
            eprintln!("elesdee: update install failed: {error}");
            crate::start_core_after_update(&app);
            set_state(
                &app,
                next_state(UpdateState::Installing, UpdateSignal::InstallFailed),
                Some(install_error(&error)),
            );
            Ok(current_snapshot(&app))
        }
    }
}

/// Records the update state and starts the periodic checks.
pub fn setup(app: &AppHandle) {
    let configured = updater_pubkey().is_some();
    let snapshot = initial_snapshot(&app.package_info().version.to_string(), configured);
    app.manage(UpdateManager(Mutex::new(Runtime {
        snapshot: Some(snapshot),
        pending: None,
    })));
    if !configured {
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            tauri::async_runtime::block_on(check_and_download(handle.clone()));
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_without_an_update_key_reports_updates_unavailable() {
        let snapshot = initial_snapshot("1.2.3", false);
        assert_eq!(snapshot.state, UpdateState::Unavailable);
        assert_eq!(snapshot.current_version, "1.2.3");
        assert_eq!(snapshot.error, Some(UpdateError::NotConfigured));
    }

    #[test]
    fn a_configured_build_starts_idle() {
        let snapshot = initial_snapshot("1.2.3", true);
        assert_eq!(snapshot.state, UpdateState::Idle);
        assert_eq!(snapshot.error, None);
    }

    #[test]
    fn a_found_update_moves_from_checking_to_downloading() {
        assert_eq!(
            next_state(UpdateState::Idle, UpdateSignal::CheckStarted),
            UpdateState::Checking
        );
        assert_eq!(
            next_state(UpdateState::Checking, UpdateSignal::Found),
            UpdateState::Downloading
        );
    }

    #[test]
    fn no_update_reports_the_latest_version() {
        assert_eq!(
            next_state(UpdateState::Checking, UpdateSignal::NoUpdate),
            UpdateState::UpToDate
        );
    }

    #[test]
    fn a_failed_check_returns_to_error_and_can_retry() {
        assert_eq!(
            next_state(UpdateState::Checking, UpdateSignal::CheckFailed),
            UpdateState::Error
        );
        assert_eq!(
            next_state(UpdateState::Error, UpdateSignal::CheckStarted),
            UpdateState::Checking
        );
    }

    #[test]
    fn a_download_ends_ready_or_in_error() {
        assert_eq!(
            next_state(UpdateState::Downloading, UpdateSignal::Downloaded),
            UpdateState::Ready
        );
        assert_eq!(
            next_state(UpdateState::Downloading, UpdateSignal::DownloadFailed),
            UpdateState::Error
        );
    }

    #[test]
    fn an_install_starts_when_no_session_runs() {
        assert_eq!(
            next_state(
                UpdateState::Ready,
                UpdateSignal::InstallRequested {
                    sessions_running: false,
                    restart_confirmed: false,
                },
            ),
            UpdateState::Installing
        );
    }

    #[test]
    fn a_running_session_defers_the_install_until_the_user_agrees() {
        let request = UpdateSignal::InstallRequested {
            sessions_running: true,
            restart_confirmed: false,
        };
        assert_eq!(
            next_state(UpdateState::Ready, request),
            UpdateState::Deferred
        );
    }

    #[test]
    fn a_confirmed_restart_installs_while_sessions_run() {
        assert_eq!(
            next_state(
                UpdateState::Ready,
                UpdateSignal::InstallRequested {
                    sessions_running: true,
                    restart_confirmed: true,
                },
            ),
            UpdateState::Installing
        );
        assert_eq!(
            next_state(
                UpdateState::Deferred,
                UpdateSignal::InstallRequested {
                    sessions_running: true,
                    restart_confirmed: true,
                },
            ),
            UpdateState::Installing
        );
    }

    #[test]
    fn a_deferred_update_installs_when_sessions_close() {
        assert_eq!(
            next_state(UpdateState::Deferred, UpdateSignal::SessionsClosed),
            UpdateState::Installing
        );
        assert_eq!(
            next_state(
                UpdateState::Deferred,
                UpdateSignal::InstallRequested {
                    sessions_running: false,
                    restart_confirmed: false,
                },
            ),
            UpdateState::Installing
        );
    }

    #[test]
    fn a_deferred_update_waits_while_sessions_stay_open() {
        assert_eq!(
            next_state(UpdateState::Deferred, UpdateSignal::CheckStarted),
            UpdateState::Deferred
        );
    }

    #[test]
    fn a_failed_install_reports_an_error() {
        assert_eq!(
            next_state(UpdateState::Installing, UpdateSignal::InstallFailed),
            UpdateState::Error
        );
    }

    #[test]
    fn an_unconfigured_build_stays_unavailable() {
        assert_eq!(
            next_state(UpdateState::Unavailable, UpdateSignal::CheckStarted),
            UpdateState::Unavailable
        );
    }
}
