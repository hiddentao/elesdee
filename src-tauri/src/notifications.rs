//! Desktop notifications for sessions that need the user.
//!
//! The UI sends the event and the text. This module decides whether the
//! notification would repeat what the focused window already shows, and it
//! routes a click back to the session that sent it.

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

/// The event the UI listens for when the user clicks a notification.
pub const NOTIFICATION_CLICKED_EVENT: &str = "elesdee://notification-clicked";
const MAIN_WINDOW: &str = "main";
const SESSION_KEY: &str = "sessionId";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum NotificationKind {
    Waiting,
    Done,
    Message,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScreenState {
    pub window_focused: bool,
    pub session_visible: bool,
}

/// Returns true when the event is not already on the screen.
///
/// A waiting session shows its card, and a done session shows its status, in
/// the focused window, so a desktop notification would repeat them. A message
/// becomes a toast in the window, so the focused window always covers it.
pub fn should_notify(kind: NotificationKind, screen: ScreenState) -> bool {
    match kind {
        NotificationKind::Waiting | NotificationKind::Done => {
            !(screen.window_focused && screen.session_visible)
        }
        NotificationKind::Message => !screen.window_focused,
    }
}

fn permission_name(state: PermissionState) -> &'static str {
    match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Prompt => "prompt",
        PermissionState::PromptWithRationale => "promptWithRationale",
    }
}

#[tauri::command]
pub fn notification_permission_state(app: AppHandle) -> Result<String, String> {
    app.notification()
        .permission_state()
        .map(permission_name)
        .map(str::to_owned)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn request_notification_permission(app: AppHandle) -> Result<String, String> {
    app.notification()
        .request_permission()
        .map(permission_name)
        .map(str::to_owned)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn show_notification(
    app: AppHandle,
    session_id: String,
    title: String,
    body: String,
    kind: NotificationKind,
    session_visible: bool,
) -> Result<bool, String> {
    let window_focused = app
        .get_webview_window(MAIN_WINDOW)
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    let screen = ScreenState {
        window_focused,
        session_visible,
    };
    if !should_notify(kind, screen) {
        return Ok(false);
    }
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .extra(SESSION_KEY, session_id)
        .show()
        .map_err(|error| error.to_string())?;
    Ok(true)
}

/// Sends a click on a notification to the UI and brings the window forward.
pub fn register_click_handler(app: &AppHandle) {
    let handle = app.clone();
    if let Err(error) = app.notification().on_action(move |action| {
        if action.action_id() != "tap" {
            return;
        }
        let Some(notification) = action.notification() else {
            return;
        };
        let Some(session_id) = notification
            .extra()
            .get(SESSION_KEY)
            .and_then(|value| value.as_str())
        else {
            return;
        };
        focus_main_window(&handle);
        let _ = handle.emit(NOTIFICATION_CLICKED_EVENT, session_id.to_owned());
    }) {
        eprintln!("elesdee: could not listen for notification clicks: {error}");
    }
}

fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::{should_notify, NotificationKind, ScreenState};

    const FOCUSED: ScreenState = ScreenState {
        window_focused: true,
        session_visible: true,
    };

    #[test]
    fn skips_a_waiting_session_the_window_already_shows() {
        assert!(!should_notify(NotificationKind::Waiting, FOCUSED));
    }

    #[test]
    fn shows_a_waiting_session_that_is_not_on_screen() {
        let other_session = ScreenState {
            session_visible: false,
            ..FOCUSED
        };
        let window_behind = ScreenState {
            window_focused: false,
            ..FOCUSED
        };
        assert!(should_notify(NotificationKind::Waiting, other_session));
        assert!(should_notify(NotificationKind::Waiting, window_behind));
    }

    #[test]
    fn skips_a_message_in_the_focused_window() {
        assert!(!should_notify(NotificationKind::Message, FOCUSED));
        let window_behind = ScreenState {
            window_focused: false,
            ..FOCUSED
        };
        assert!(should_notify(NotificationKind::Message, window_behind));
    }

    #[test]
    fn skips_a_done_notice_the_window_already_shows() {
        assert!(!should_notify(NotificationKind::Done, FOCUSED));
        let other_session = ScreenState {
            session_visible: false,
            ..FOCUSED
        };
        let window_behind = ScreenState::default();
        assert!(should_notify(NotificationKind::Done, other_session));
        assert!(should_notify(NotificationKind::Done, window_behind));
    }
}
