//! Local notifications for sessions that need the user.
//!
//! The screens decide which event needs a notice. This module owns the
//! permission answer, the one local notification for that event and the route
//! back to its session when the user taps it. A notice never holds the shell
//! state lock and never opens a connection, so the event stream keeps running.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

/// The Tauri event that carries a tap on a local notification.
pub const NOTIFICATION_CLICKED_EVENT: &str = "phone:notification-click";
const MAIN_WINDOW: &str = "main";
const SESSION_KEY: &str = "sessionId";
const COMPUTER_KEY: &str = "computerId";
const TAP_ACTION: &str = "tap";
/// The number of recent notifications whose route is kept in memory.
const MAX_ROUTES: usize = 64;

/// One local notification. The screens supply the text from the catalogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNotice {
    pub computer_id: String,
    pub session_id: String,
    pub title: String,
    pub body: String,
}

/// The session to open after a tap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTarget {
    pub computer_id: String,
    pub session_id: String,
}

/// Remembers which session each recent local notification belongs to.
///
/// iOS returns no extra data with a tap, so the shell routes by the
/// notification id there. Android carries the ids in the extra data, and the
/// id table is the fallback.
#[derive(Default)]
pub struct NotificationRoutes {
    inner: Mutex<RouteMap>,
}

#[derive(Default)]
struct RouteMap {
    next: i32,
    targets: HashMap<i32, NotificationTarget>,
}

impl NotificationRoutes {
    fn remember(&self, target: NotificationTarget) -> Result<i32, String> {
        let mut map = self
            .inner
            .lock()
            .map_err(|_| "The notification routes are unavailable.".to_owned())?;
        Ok(map.remember(target))
    }

    fn target(&self, id: i32) -> Option<NotificationTarget> {
        self.inner.lock().ok()?.targets.get(&id).cloned()
    }
}

impl RouteMap {
    fn remember(&mut self, target: NotificationTarget) -> i32 {
        let id = self.next.max(1);
        self.next = if id == i32::MAX { 1 } else { id + 1 };
        if self.targets.len() >= MAX_ROUTES
            && let Some(oldest) = self.targets.keys().min().copied()
        {
            self.targets.remove(&oldest);
        }
        self.targets.insert(id, target);
        id
    }
}

/// The platform operations that the decisions need.
///
/// A real host wraps the notification plugin. The tests use a fake, so a
/// refused permission is checked without a device.
pub trait NotificationHost {
    fn permission(&self) -> Result<PermissionState, String>;
    fn request(&self) -> Result<PermissionState, String>;
    fn display(&self, notice: &LocalNotice) -> Result<(), String>;
}

fn permission_name(state: PermissionState) -> &'static str {
    match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Prompt => "prompt",
        PermissionState::PromptWithRationale => "promptWithRationale",
    }
}

/// Checks the permission and asks for it while the platform still prompts.
///
/// The answer always reaches the screens, so a refusal shows in the shell
/// instead of stopping the session list.
pub fn ensure_permission<H: NotificationHost>(host: &H) -> Result<&'static str, String> {
    let state = host.permission()?;
    let state = match state {
        PermissionState::Prompt | PermissionState::PromptWithRationale => host.request()?,
        other => other,
    };
    Ok(permission_name(state))
}

/// Shows one local notice when the platform allows it.
///
/// A refused permission is a state, not an error. The shell shows that
/// notifications are off and every session keeps working.
pub fn show_notice<H: NotificationHost>(host: &H, notice: &LocalNotice) -> Result<bool, String> {
    if host.permission()? != PermissionState::Granted {
        return Ok(false);
    }
    host.display(notice)?;
    Ok(true)
}

/// The notice that an event earns after the rules are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeDecision {
    /// A session waits for permission, input or a session start.
    Waiting(&'static str),
    /// An agent reported its task done.
    Done,
    /// An agent sent a message.
    Message,
}

/// The phone screen state at the moment an event arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenState<'a> {
    /// The session id on the open session screen, or `None`.
    pub session_id: Option<&'a str>,
    /// True when the app is in the foreground.
    pub app_visible: bool,
}

/// The text that the package sends for each kind of notice.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeTexts {
    pub waiting_title: String,
    pub request_title: String,
    pub request_body: String,
    pub permission_body: String,
    pub question_body: String,
    pub input_body: String,
    pub done_title: String,
    pub done_body: String,
    pub message_title: String,
    pub message_body: String,
}

impl NoticeTexts {
    /// Returns the title and body that fit one decision.
    pub fn for_decision(&self, decision: NoticeDecision) -> (String, String) {
        match decision {
            NoticeDecision::Waiting("permission") => {
                (cap(&self.waiting_title), cap(&self.permission_body))
            }
            NoticeDecision::Waiting("question") => {
                (cap(&self.waiting_title), cap(&self.question_body))
            }
            NoticeDecision::Waiting("start") => (cap(&self.request_title), cap(&self.request_body)),
            NoticeDecision::Waiting(_) => (cap(&self.waiting_title), cap(&self.input_body)),
            NoticeDecision::Done => (cap(&self.done_title), cap(&self.done_body)),
            NoticeDecision::Message => (cap(&self.message_title), cap(&self.message_body)),
        }
    }
}

const MAX_NOTICE_TEXT: usize = 512;

fn cap(value: &str) -> String {
    value.chars().take(MAX_NOTICE_TEXT).collect()
}

fn safe_id(value: &str) -> Option<&str> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return None;
    }
    Some(value)
}

fn field<'a>(event: &'a Value, name: &str) -> Option<&'a str> {
    event.get(name)?.as_str()
}

fn event_session_id(event: &Value) -> Option<&str> {
    safe_id(field(event, "sessionId")?)
}

/// Decides which event raises a notification.
///
/// A session update notifies when its waiting state changes, except for a
/// question: the pending question event that follows carries that notice. A
/// pending session request, a question, a done report and a message each raise
/// their own notice. A notice for the open session stays quiet while the app
/// is in front, and a message always sends.
pub fn notice_for_event(event: &Value, screen: ScreenState<'_>) -> Option<NoticeDecision> {
    let session_id = event_session_id(event)?;
    let decision = match field(event, "kind")? {
        "session" => {
            if field(event, "status")? != "active" {
                return None;
            }
            let waiting = field(event, "waitingFor")?;
            if waiting == "question" {
                return None;
            }
            if field(event, "previousWaitingFor") == Some(waiting) {
                return None;
            }
            NoticeDecision::Waiting(match waiting {
                "permission" => "permission",
                "question" => "question",
                "start" => "start",
                _ => "input",
            })
        }
        "question" => {
            if field(event, "status")? != "pending" {
                return None;
            }
            NoticeDecision::Waiting("question")
        }
        "request" => {
            if field(event, "status")? != "pending" {
                return None;
            }
            NoticeDecision::Waiting("start")
        }
        "notification" => match field(event, "notificationKind")? {
            "done" => NoticeDecision::Done,
            "message" => NoticeDecision::Message,
            _ => return None,
        },
        _ => return None,
    };
    if decision != NoticeDecision::Message
        && screen.app_visible
        && screen.session_id == Some(session_id)
    {
        return None;
    }
    Some(decision)
}

/// Raises one notice from a package event when the rules allow it.
#[tauri::command]
pub async fn phone_notification_event(
    app: AppHandle,
    computer_id: String,
    event: Value,
    screen_session_id: Option<String>,
    app_visible: bool,
    texts: NoticeTexts,
) -> Result<bool, String> {
    let screen = ScreenState {
        session_id: screen_session_id.as_deref(),
        app_visible,
    };
    let Some(decision) = notice_for_event(&event, screen) else {
        return Ok(false);
    };
    let session_id =
        event_session_id(&event).ok_or_else(|| "The event has no session.".to_owned())?;
    if safe_id(&computer_id).is_none() {
        return Err("The computer id is not valid.".to_owned());
    }
    let (title, body) = texts.for_decision(decision);
    let notice = LocalNotice {
        computer_id,
        session_id: session_id.to_owned(),
        title,
        body,
    };
    show_notice(&PluginHost(app), &notice)
}

/// Reads the computer and session from a tap on a notification.
pub fn notification_target(
    action_id: &str,
    extra: &HashMap<String, Value>,
) -> Option<NotificationTarget> {
    if action_id != TAP_ACTION {
        return None;
    }
    let computer_id = extra.get(COMPUTER_KEY)?.as_str()?.to_owned();
    let session_id = extra.get(SESSION_KEY)?.as_str()?.to_owned();
    if computer_id.is_empty() || session_id.is_empty() {
        return None;
    }
    Some(NotificationTarget {
        computer_id,
        session_id,
    })
}

/// The plugin-backed host. It touches no shell state and waits on no network.
struct PluginHost(AppHandle);

impl NotificationHost for PluginHost {
    fn permission(&self) -> Result<PermissionState, String> {
        self.0
            .notification()
            .permission_state()
            .map_err(|error| error.to_string())
    }

    fn request(&self) -> Result<PermissionState, String> {
        self.0
            .notification()
            .request_permission()
            .map_err(|error| error.to_string())
    }

    fn display(&self, notice: &LocalNotice) -> Result<(), String> {
        let target = NotificationTarget {
            computer_id: notice.computer_id.clone(),
            session_id: notice.session_id.clone(),
        };
        let id = self.0.state::<NotificationRoutes>().remember(target)?;
        self.0
            .notification()
            .builder()
            .id(id)
            .title(&notice.title)
            .body(&notice.body)
            .extra(SESSION_KEY, &notice.session_id)
            .extra(COMPUTER_KEY, &notice.computer_id)
            .show()
            .map_err(|error| error.to_string())
    }
}

/// Returns the current notification permission for the shell.
#[tauri::command]
pub async fn phone_notification_state(app: AppHandle) -> Result<String, String> {
    PluginHost(app)
        .permission()
        .map(permission_name)
        .map(str::to_owned)
}

/// Asks for the permission while the platform still prompts.
#[tauri::command]
pub async fn phone_notification_request(app: AppHandle) -> Result<String, String> {
    ensure_permission(&PluginHost(app)).map(str::to_owned)
}

/// Raises one local notification for a session.
#[tauri::command]
pub async fn phone_notification_show(
    app: AppHandle,
    computer_id: String,
    session_id: String,
    title: String,
    body: String,
) -> Result<bool, String> {
    let notice = LocalNotice {
        computer_id,
        session_id,
        title,
        body,
    };
    show_notice(&PluginHost(app), &notice)
}

/// Sends a tap on a notification to the screens.
pub fn register_click_handler(app: &AppHandle) {
    let handle = app.clone();
    if let Err(error) = app.notification().on_action(move |action| {
        if action.action_id() != TAP_ACTION {
            return;
        }
        let Some(notification) = action.notification() else {
            return;
        };
        let target = notification_target(action.action_id(), notification.extra()).or_else(|| {
            handle
                .state::<NotificationRoutes>()
                .target(notification.id())
        });
        let Some(target) = target else {
            return;
        };
        focus_main_window(&handle);
        if let Err(error) = handle.emit(NOTIFICATION_CLICKED_EVENT, target) {
            tracing::warn!(%error, "could not emit a notification click");
        }
    }) {
        tracing::warn!(%error, "could not listen for notification clicks");
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
    use super::{
        LocalNotice, MAX_ROUTES, NoticeDecision, NoticeTexts, NotificationHost, NotificationRoutes,
        NotificationTarget, ScreenState, ensure_permission, notice_for_event, notification_target,
        show_notice,
    };
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use tauri_plugin_notification::PermissionState;

    struct FakeHost {
        state: Cell<PermissionState>,
        request_state: PermissionState,
        requested: Cell<bool>,
        shown: RefCell<Vec<LocalNotice>>,
    }

    impl FakeHost {
        fn new(state: PermissionState) -> Self {
            Self {
                state: Cell::new(state),
                request_state: state,
                requested: Cell::new(false),
                shown: RefCell::new(Vec::new()),
            }
        }

        fn refusing() -> Self {
            let mut host = Self::new(PermissionState::Prompt);
            host.request_state = PermissionState::Denied;
            host
        }
    }

    impl NotificationHost for FakeHost {
        fn permission(&self) -> Result<PermissionState, String> {
            Ok(self.state.get())
        }

        fn request(&self) -> Result<PermissionState, String> {
            self.requested.set(true);
            self.state.set(self.request_state);
            Ok(self.request_state)
        }

        fn display(&self, notice: &LocalNotice) -> Result<(), String> {
            self.shown.borrow_mut().push(notice.clone());
            Ok(())
        }
    }

    fn notice() -> LocalNotice {
        LocalNotice {
            computer_id: "computer-1".to_owned(),
            session_id: "session-1".to_owned(),
            title: "Build needs you".to_owned(),
            body: "Approve or deny a request.".to_owned(),
        }
    }

    fn extra() -> HashMap<String, serde_json::Value> {
        HashMap::from([
            ("computerId".to_owned(), json!("computer-1")),
            ("sessionId".to_owned(), json!("session-1")),
        ])
    }

    #[test]
    fn a_refused_permission_keeps_notifications_off() {
        let host = FakeHost::new(PermissionState::Denied);
        assert_eq!(ensure_permission(&host).expect("permission"), "denied");
        assert!(!host.requested.get());
        assert!(!show_notice(&host, &notice()).expect("show"));
        assert!(host.shown.borrow().is_empty());
    }

    #[test]
    fn a_prompt_is_asked_once_and_reports_the_refusal() {
        let host = FakeHost::refusing();
        assert_eq!(ensure_permission(&host).expect("permission"), "denied");
        assert!(host.requested.get());
        assert!(!show_notice(&host, &notice()).expect("show"));
        assert!(host.shown.borrow().is_empty());
    }

    #[test]
    fn a_granted_permission_shows_one_notice() {
        let host = FakeHost::new(PermissionState::Granted);
        assert!(show_notice(&host, &notice()).expect("show"));
        let shown = host.shown.borrow();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].computer_id, "computer-1");
        assert_eq!(shown[0].session_id, "session-1");
        assert_eq!(shown[0].title, "Build needs you");
        assert_eq!(shown[0].body, "Approve or deny a request.");
    }

    #[test]
    fn a_tap_routes_to_its_computer_and_session() {
        let target = notification_target("tap", &extra()).expect("target");
        assert_eq!(target.computer_id, "computer-1");
        assert_eq!(target.session_id, "session-1");
    }

    #[test]
    fn recent_notifications_keep_their_route_for_a_tap() {
        let routes = NotificationRoutes::default();
        let target = NotificationTarget {
            computer_id: "computer-1".to_owned(),
            session_id: "session-1".to_owned(),
        };
        let id = routes.remember(target.clone()).expect("route id");
        assert_eq!(id, 1);
        assert_eq!(routes.target(id), Some(target));
        assert_eq!(routes.target(id + 1), None);
    }

    #[test]
    fn the_route_table_keeps_only_the_newest_notifications() {
        let routes = NotificationRoutes::default();
        let first = routes
            .remember(NotificationTarget {
                computer_id: "computer-1".to_owned(),
                session_id: "first".to_owned(),
            })
            .expect("route id");
        for _ in 0..MAX_ROUTES {
            routes
                .remember(NotificationTarget {
                    computer_id: "computer-1".to_owned(),
                    session_id: "next".to_owned(),
                })
                .expect("route id");
        }
        assert_eq!(routes.target(first), None);
    }

    #[test]
    fn other_actions_and_incomplete_extra_are_ignored() {
        assert!(notification_target("dismiss", &extra()).is_none());
        assert!(notification_target("tap", &HashMap::new()).is_none());
        assert!(
            notification_target(
                "tap",
                &HashMap::from([("computerId".to_owned(), json!("computer-1"))])
            )
            .is_none()
        );
        assert!(
            notification_target(
                "tap",
                &HashMap::from([
                    ("computerId".to_owned(), json!("")),
                    ("sessionId".to_owned(), json!("session-1")),
                ])
            )
            .is_none()
        );
    }

    fn idle_screen() -> ScreenState<'static> {
        ScreenState {
            session_id: None,
            app_visible: false,
        }
    }

    #[test]
    fn a_waiting_transition_earns_a_notice() {
        let event = json!({
            "kind": "session",
            "sessionId": "s1",
            "sessionName": "Build",
            "status": "active",
            "waitingFor": "permission",
            "previousWaitingFor": "working",
        });
        assert_eq!(
            notice_for_event(&event, idle_screen()),
            Some(NoticeDecision::Waiting("permission"))
        );
    }

    #[test]
    fn an_unchanged_waiting_state_stays_quiet() {
        let event = json!({
            "kind": "session",
            "sessionId": "s1",
            "sessionName": "Build",
            "status": "active",
            "waitingFor": "input",
            "previousWaitingFor": "input",
        });
        assert_eq!(notice_for_event(&event, idle_screen()), None);
    }

    #[test]
    fn a_question_update_owns_the_question_notice() {
        let session = json!({
            "kind": "session",
            "sessionId": "s1",
            "sessionName": "Build",
            "status": "active",
            "waitingFor": "question",
            "previousWaitingFor": null,
        });
        assert_eq!(notice_for_event(&session, idle_screen()), None);

        let question = json!({
            "kind": "question",
            "sessionId": "s1",
            "sessionName": "Build",
            "status": "pending",
        });
        assert_eq!(
            notice_for_event(&question, idle_screen()),
            Some(NoticeDecision::Waiting("question"))
        );
    }

    #[test]
    fn a_pending_session_request_earns_a_start_notice() {
        let event = json!({
            "kind": "request",
            "sessionId": "s1",
            "sessionName": "Build",
            "status": "pending",
        });
        assert_eq!(
            notice_for_event(&event, idle_screen()),
            Some(NoticeDecision::Waiting("start"))
        );
    }

    #[test]
    fn done_and_message_notifications_earn_their_notice() {
        let done = json!({
            "kind": "notification",
            "sessionId": "s1",
            "sessionName": "Build",
            "notificationKind": "done",
            "message": "",
        });
        assert_eq!(
            notice_for_event(&done, idle_screen()),
            Some(NoticeDecision::Done)
        );

        let message = json!({
            "kind": "notification",
            "sessionId": "s1",
            "sessionName": "Build",
            "notificationKind": "message",
            "message": "Please look",
        });
        assert_eq!(
            notice_for_event(&message, idle_screen()),
            Some(NoticeDecision::Message)
        );
    }

    #[test]
    fn the_open_session_suppresses_a_waiting_notice() {
        let event = json!({
            "kind": "session",
            "sessionId": "s1",
            "sessionName": "Build",
            "status": "active",
            "waitingFor": "input",
            "previousWaitingFor": null,
        });
        let screen = ScreenState {
            session_id: Some("s1"),
            app_visible: true,
        };
        assert_eq!(notice_for_event(&event, screen), None);
    }

    #[test]
    fn a_message_is_never_suppressed() {
        let event = json!({
            "kind": "notification",
            "sessionId": "s1",
            "sessionName": "Build",
            "notificationKind": "message",
            "message": "Please look",
        });
        let screen = ScreenState {
            session_id: Some("s1"),
            app_visible: true,
        };
        assert_eq!(
            notice_for_event(&event, screen),
            Some(NoticeDecision::Message)
        );
    }

    #[test]
    fn an_unknown_event_earns_nothing() {
        assert_eq!(
            notice_for_event(&json!({"kind": "other"}), idle_screen()),
            None
        );
        assert_eq!(
            notice_for_event(
                &json!({
                    "kind": "session",
                    "sessionId": "../state",
                    "status": "active",
                }),
                idle_screen()
            ),
            None
        );
    }

    #[test]
    fn the_texts_follow_the_decision() {
        let texts = NoticeTexts {
            waiting_title: "Waiting".to_owned(),
            request_title: "Request".to_owned(),
            request_body: "Approve".to_owned(),
            permission_body: "Permission".to_owned(),
            question_body: "Question".to_owned(),
            input_body: "Input".to_owned(),
            done_title: "Done".to_owned(),
            done_body: "Complete".to_owned(),
            message_title: "Message".to_owned(),
            message_body: "Hello".to_owned(),
        };
        assert_eq!(
            texts.for_decision(NoticeDecision::Waiting("permission")),
            ("Waiting".to_owned(), "Permission".to_owned())
        );
        assert_eq!(
            texts.for_decision(NoticeDecision::Waiting("start")),
            ("Request".to_owned(), "Approve".to_owned())
        );
        assert_eq!(
            texts.for_decision(NoticeDecision::Message),
            ("Message".to_owned(), "Hello".to_owned())
        );
    }
}
