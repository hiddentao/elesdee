use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    InsertTextParams, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::log::{EnableParams as LogEnableParams, EventEntryAdded};
use chromiumoxide::cdp::browser_protocol::network::{
    EnableParams as NetworkEnableParams, EventLoadingFailed, EventRequestWillBeSent,
    EventResponseReceived,
};
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, GetNavigationHistoryParams, NavigateToHistoryEntryParams,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    EnableParams as RuntimeEnableParams, EventConsoleApiCalled, EventExceptionThrown, RemoteObject,
};
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde_json::Value;
use tokio::task::JoinHandle;

use super::logs::BrowserLogs;
use super::{BrowserError, PageInfo};

const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL: Duration = Duration::from_millis(40);
const ACTION_SETTLE: Duration = Duration::from_millis(200);

/// A per-session Chromium process.
#[derive(Clone)]
pub struct Chromium {
    browser: Arc<Browser>,
    handler: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Chromium {
    pub async fn launch(profile: &Path, executable: &Path) -> Result<Self, BrowserError> {
        crate::private_fs::create_private_dir_all(profile).map_err(|error| {
            BrowserError::Unavailable(format!(
                "The browser profile folder could not be created: {error}"
            ))
        })?;
        let config = BrowserConfig::builder()
            .chrome_executable(executable)
            .user_data_dir(profile)
            .new_headless_mode()
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--password-store=basic")
            .build()
            .map_err(BrowserError::Unavailable)?;
        let (browser, mut handler) = Browser::launch(config).await.map_err(|error| {
            BrowserError::Unavailable(format!("Chromium did not start: {error}"))
        })?;
        let task = tokio::spawn(async move { while handler.next().await.is_some() {} });
        Ok(Self {
            browser: Arc::new(browser),
            handler: Arc::new(Mutex::new(Some(task))),
        })
    }

    pub async fn stop(self) {
        if let Some(task) = self
            .handler
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        {
            task.abort();
        }
        // Dropping the last browser handle ends the Chromium process.
    }

    pub async fn new_page(&self) -> Result<Page, String> {
        self.browser
            .new_page("about:blank")
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn goto(&self, page: &Page, url: &str) -> Result<PageInfo, String> {
        page.goto(url).await.map_err(|error| error.to_string())?;
        wait_ready(page).await;
        page_info(page).await
    }

    pub async fn back(&self, page: &Page) -> Result<PageInfo, String> {
        self.move_history(page, -1).await
    }

    pub async fn forward(&self, page: &Page) -> Result<PageInfo, String> {
        self.move_history(page, 1).await
    }

    async fn move_history(&self, page: &Page, direction: i64) -> Result<PageInfo, String> {
        let history = page
            .execute(GetNavigationHistoryParams::default())
            .await
            .map_err(|error| error.to_string())?
            .result;
        let target = history.current_index + direction;
        if target < 0 || target as usize >= history.entries.len() {
            return page_info(page).await;
        }
        let entry_id = history.entries[target as usize].id;
        page.execute(NavigateToHistoryEntryParams::new(entry_id))
            .await
            .map_err(|error| error.to_string())?;
        tokio::time::sleep(ACTION_SETTLE).await;
        wait_ready(page).await;
        page_info(page).await
    }

    pub async fn reload(&self, page: &Page) -> Result<PageInfo, String> {
        page.reload().await.map_err(|error| error.to_string())?;
        wait_ready(page).await;
        page_info(page).await
    }

    pub async fn text(&self, page: &Page) -> Result<String, String> {
        let result = page
            .evaluate("document.body ? document.body.innerText : ''")
            .await
            .map_err(|error| error.to_string())?;
        Ok(result
            .value()
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned())
    }

    pub async fn click(&self, page: &Page, selector: &str) -> Result<PageInfo, String> {
        let element = page
            .find_element(selector)
            .await
            .map_err(|_| format!("No element matches the selector {selector}."))?;
        element.click().await.map_err(|error| error.to_string())?;
        tokio::time::sleep(ACTION_SETTLE).await;
        wait_ready(page).await;
        page_info(page).await
    }

    pub async fn type_text(
        &self,
        page: &Page,
        selector: &str,
        text: &str,
    ) -> Result<PageInfo, String> {
        let element = page
            .find_element(selector)
            .await
            .map_err(|_| format!("No element matches the selector {selector}."))?;
        element.focus().await.map_err(|error| error.to_string())?;
        element
            .type_str(text)
            .await
            .map_err(|error| error.to_string())?;
        page_info(page).await
    }

    /// Clicks at a point given as a fraction of the viewport. The screenshot
    /// and the page use the same viewport, so the fraction maps to both.
    pub async fn click_at(&self, page: &Page, x: f64, y: f64) -> Result<PageInfo, String> {
        let width = viewport_size(page, "window.innerWidth", 800.0).await;
        let height = viewport_size(page, "window.innerHeight", 600.0).await;
        let px = (x * width).clamp(0.0, (width - 1.0).max(0.0));
        let py = (y * height).clamp(0.0, (height - 1.0).max(0.0));

        page.execute(DispatchMouseEventParams::new(
            DispatchMouseEventType::MouseMoved,
            px,
            py,
        ))
        .await
        .map_err(|error| error.to_string())?;

        let mut pressed =
            DispatchMouseEventParams::new(DispatchMouseEventType::MousePressed, px, py);
        pressed.button = Some(MouseButton::Left);
        pressed.buttons = Some(1);
        pressed.click_count = Some(1);
        page.execute(pressed)
            .await
            .map_err(|error| error.to_string())?;

        let mut released =
            DispatchMouseEventParams::new(DispatchMouseEventType::MouseReleased, px, py);
        released.button = Some(MouseButton::Left);
        released.buttons = Some(0);
        released.click_count = Some(1);
        page.execute(released)
            .await
            .map_err(|error| error.to_string())?;

        tokio::time::sleep(ACTION_SETTLE).await;
        wait_ready(page).await;
        page_info(page).await
    }

    /// Inserts text at the focused element, as the live view does for typed
    /// characters.
    pub async fn type_live(&self, page: &Page, text: &str) -> Result<PageInfo, String> {
        page.execute(InsertTextParams::new(text))
            .await
            .map_err(|error| error.to_string())?;
        tokio::time::sleep(ACTION_SETTLE).await;
        page_info(page).await
    }

    /// Presses a named key, a letter or a digit. A command modifier makes the
    /// event a shortcut instead of text, so the page receives the combination.
    pub async fn press_key(
        &self,
        page: &Page,
        key: &str,
        modifiers: u8,
    ) -> Result<PageInfo, String> {
        let definition = key_definition(key, modifiers)
            .ok_or_else(|| format!("The live view cannot press the key {key}."))?;
        let down = if definition.text.is_some() {
            DispatchKeyEventType::KeyDown
        } else {
            DispatchKeyEventType::RawKeyDown
        };
        let commands = if cfg!(target_os = "macos") {
            editing_commands(&definition.code, modifiers)
        } else {
            &[]
        };
        let mut pressed = DispatchKeyEventParams::new(down);
        pressed.modifiers = Some(i64::from(modifiers));
        pressed.key = Some(definition.key.clone());
        pressed.code = Some(definition.code.clone());
        pressed.windows_virtual_key_code = Some(definition.key_code);
        pressed.native_virtual_key_code = Some(definition.key_code);
        if !commands.is_empty() {
            pressed.commands = Some(
                commands
                    .iter()
                    .map(|command| (*command).to_owned())
                    .collect(),
            );
        }
        if let Some(text) = definition.text {
            pressed.text = Some(text.clone());
            pressed.unmodified_text = Some(text);
        }
        page.execute(pressed)
            .await
            .map_err(|error| error.to_string())?;

        let mut released = DispatchKeyEventParams::new(DispatchKeyEventType::KeyUp);
        released.modifiers = Some(i64::from(modifiers));
        released.key = Some(definition.key);
        released.code = Some(definition.code);
        released.windows_virtual_key_code = Some(definition.key_code);
        released.native_virtual_key_code = Some(definition.key_code);
        page.execute(released)
            .await
            .map_err(|error| error.to_string())?;

        tokio::time::sleep(ACTION_SETTLE).await;
        page_info(page).await
    }

    /// Starts the console, log and network domains for one page.
    pub async fn enable_logs(&self, page: &Page) -> Result<(), String> {
        page.execute(RuntimeEnableParams::default())
            .await
            .map_err(|error| error.to_string())?;
        page.execute(LogEnableParams::default())
            .await
            .map_err(|error| error.to_string())?;
        page.execute(NetworkEnableParams::default())
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Collects console messages and network requests into one shared buffer.
    ///
    /// The listeners belong to the browser process, so the caller registers
    /// them once per session. Every page in that process reports into the same
    /// buffer, which is the session's own browser.
    pub async fn subscribe_logs(
        &self,
        page: &Page,
        logs: Arc<Mutex<BrowserLogs>>,
    ) -> Result<(), String> {
        let mut console = page
            .event_listener::<EventConsoleApiCalled>()
            .await
            .map_err(|error| error.to_string())?;
        let mut exceptions = page
            .event_listener::<EventExceptionThrown>()
            .await
            .map_err(|error| error.to_string())?;
        let mut entries = page
            .event_listener::<EventEntryAdded>()
            .await
            .map_err(|error| error.to_string())?;
        let mut requests = page
            .event_listener::<EventRequestWillBeSent>()
            .await
            .map_err(|error| error.to_string())?;
        let mut responses = page
            .event_listener::<EventResponseReceived>()
            .await
            .map_err(|error| error.to_string())?;
        let mut failures = page
            .event_listener::<EventLoadingFailed>()
            .await
            .map_err(|error| error.to_string())?;

        let console_logs = Arc::clone(&logs);
        tokio::spawn(async move {
            while let Some(event) = console.next().await {
                let level = format!("{:?}", event.r#type);
                let text = event
                    .args
                    .iter()
                    .map(remote_text)
                    .collect::<Vec<_>>()
                    .join(" ");
                push_console(&console_logs, &level, &text);
            }
        });

        let exception_logs = Arc::clone(&logs);
        tokio::spawn(async move {
            while let Some(event) = exceptions.next().await {
                let details = &event.exception_details;
                let mut text = details.text.clone();
                if let Some(exception) = &details.exception {
                    let value = remote_text(exception);
                    if !value.is_empty() && value != text {
                        text.push_str(": ");
                        text.push_str(&value);
                    }
                }
                push_console(&exception_logs, "error", &text);
            }
        });

        let entry_logs = Arc::clone(&logs);
        tokio::spawn(async move {
            while let Some(event) = entries.next().await {
                let level = format!("{:?}", event.entry.level);
                push_console(&entry_logs, &level, &event.entry.text);
            }
        });

        let request_logs = Arc::clone(&logs);
        tokio::spawn(async move {
            while let Some(event) = requests.next().await {
                let resource_type = event
                    .r#type
                    .as_ref()
                    .map(|kind| kind.as_ref().to_owned())
                    .unwrap_or_default();
                let mut buffer = lock_logs(&request_logs);
                buffer.push_network(
                    event.request_id.as_ref(),
                    &event.request.method,
                    &event.request.url,
                    &resource_type,
                );
            }
        });

        let response_logs = Arc::clone(&logs);
        tokio::spawn(async move {
            while let Some(event) = responses.next().await {
                lock_logs(&response_logs)
                    .finish_network(event.request_id.as_ref(), event.response.status);
            }
        });

        let failure_logs = Arc::clone(&logs);
        tokio::spawn(async move {
            while let Some(event) = failures.next().await {
                lock_logs(&failure_logs).fail_network(event.request_id.as_ref(), &event.error_text);
            }
        });

        Ok(())
    }

    pub async fn wait(
        &self,
        page: &Page,
        selector: Option<&str>,
        text: Option<&str>,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(selector) = selector
                && page.find_element(selector).await.is_ok()
            {
                return Ok(());
            }
            if let Some(text) = text {
                let quoted = serde_json::to_string(text).map_err(|error| error.to_string())?;
                let expression =
                    format!("document.body ? document.body.innerText.includes({quoted}) : false");
                let result = page
                    .evaluate(expression)
                    .await
                    .map_err(|error| error.to_string())?;
                if result.value().and_then(|value| value.as_bool()) == Some(true) {
                    return Ok(());
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn screenshot(&self, page: &Page) -> Result<Vec<u8>, String> {
        page.screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .build(),
        )
        .await
        .map_err(|error| error.to_string())
    }
}

/// The DevTools modifier bits. The same order is used by the desktop panel.
const MOD_ALT: u8 = 1;
const MOD_CONTROL: u8 = 2;
const MOD_META: u8 = 4;
const MOD_SHIFT: u8 = 8;
const COMMAND_MODIFIERS: u8 = MOD_ALT | MOD_CONTROL | MOD_META;

#[derive(Debug, PartialEq, Eq)]
struct KeyDefinition {
    key: String,
    code: String,
    key_code: i64,
    text: Option<String>,
}

/// The keys that the live view can press: the named keys, the letters and the
/// digits. A command modifier suppresses the inserted text, so the page sees a
/// shortcut instead of a typed character.
fn key_definition(key: &str, modifiers: u8) -> Option<KeyDefinition> {
    let command = modifiers & COMMAND_MODIFIERS != 0;
    let shift = modifiers & MOD_SHIFT != 0;

    if key.len() == 1 {
        let base = key.chars().next()?;
        if base.is_ascii_alphabetic() {
            let upper = base.to_ascii_uppercase();
            let lower = base.to_ascii_lowercase();
            let value = if shift { upper } else { lower };
            return Some(KeyDefinition {
                key: value.to_string(),
                code: format!("Key{upper}"),
                key_code: i64::from(upper as u8),
                text: (!command).then(|| value.to_string()),
            });
        }
        if base.is_ascii_digit() {
            let value = if shift {
                shifted_digit(base).unwrap_or(base)
            } else {
                base
            };
            return Some(KeyDefinition {
                key: value.to_string(),
                code: format!("Digit{base}"),
                key_code: i64::from(base as u8),
                text: (!command).then(|| value.to_string()),
            });
        }
    }

    let (code, key_code, text) = match key {
        "Enter" => ("Enter", 13, Some("\r")),
        "Tab" => ("Tab", 9, Some("\t")),
        "Escape" => ("Escape", 27, None),
        "Backspace" => ("Backspace", 8, None),
        "Delete" => ("Delete", 46, None),
        "ArrowLeft" => ("ArrowLeft", 37, None),
        "ArrowUp" => ("ArrowUp", 38, None),
        "ArrowRight" => ("ArrowRight", 39, None),
        "ArrowDown" => ("ArrowDown", 40, None),
        "Home" => ("Home", 36, None),
        "End" => ("End", 35, None),
        "PageUp" => ("PageUp", 33, None),
        "PageDown" => ("PageDown", 34, None),
        " " => ("Space", 32, Some(" ")),
        _ => return None,
    };
    Some(KeyDefinition {
        key: key.to_owned(),
        code: code.to_owned(),
        key_code,
        text: if command {
            None
        } else {
            text.map(str::to_owned)
        },
    })
}

/// The character that the unshifted digit produces with Shift on a US layout.
fn shifted_digit(digit: char) -> Option<char> {
    Some(match digit {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        _ => return None,
    })
}

/// The editing commands that a Mac shortcut runs. The renderer receives the
/// key event, but the editor command must travel in the DevTools event as
/// well. A system that handles the shortcut itself gets no command here.
fn editing_commands(code: &str, modifiers: u8) -> &'static [&'static str] {
    const SHIFT: u8 = MOD_SHIFT;
    const CONTROL: u8 = MOD_CONTROL;
    const ALT: u8 = MOD_ALT;
    const META: u8 = MOD_META;
    const CONTROL_SHIFT: u8 = MOD_CONTROL | MOD_SHIFT;
    const ALT_SHIFT: u8 = MOD_ALT | MOD_SHIFT;
    const META_SHIFT: u8 = MOD_META | MOD_SHIFT;
    match (modifiers & (SHIFT | CONTROL | ALT | META), code) {
        (META, "KeyA") => &["selectAll"],
        (META, "KeyC") => &["copy"],
        (META, "KeyX") => &["cut"],
        (META, "KeyV") => &["paste"],
        (META, "KeyZ") => &["undo"],
        (META_SHIFT, "KeyZ") => &["redo"],
        (META, "Backspace") | (META_SHIFT, "Backspace") => &["deleteToBeginningOfLine"],
        (ALT, "Backspace") | (ALT_SHIFT, "Backspace") => &["deleteWordBackward"],
        (ALT, "Delete") | (ALT_SHIFT, "Delete") => &["deleteWordForward"],
        (ALT, "ArrowLeft") => &["moveWordLeft"],
        (ALT, "ArrowRight") => &["moveWordRight"],
        (ALT_SHIFT, "ArrowLeft") => &["moveWordLeftAndModifySelection"],
        (ALT_SHIFT, "ArrowRight") => &["moveWordRightAndModifySelection"],
        (META, "ArrowUp") => &["moveToBeginningOfDocument"],
        (META, "ArrowDown") => &["moveToEndOfDocument"],
        (META, "ArrowLeft") => &["moveToLeftEndOfLine"],
        (META, "ArrowRight") => &["moveToRightEndOfLine"],
        (META_SHIFT, "ArrowUp") => &["moveToBeginningOfDocumentAndModifySelection"],
        (META_SHIFT, "ArrowDown") => &["moveToEndOfDocumentAndModifySelection"],
        (META_SHIFT, "ArrowLeft") => &["moveToLeftEndOfLineAndModifySelection"],
        (META_SHIFT, "ArrowRight") => &["moveToRightEndOfLineAndModifySelection"],
        (SHIFT, "ArrowUp") => &["moveUpAndModifySelection"],
        (SHIFT, "ArrowDown") => &["moveDownAndModifySelection"],
        (SHIFT, "ArrowLeft") => &["moveLeftAndModifySelection"],
        (SHIFT, "ArrowRight") => &["moveRightAndModifySelection"],
        (SHIFT, "Home") => &["moveToBeginningOfDocumentAndModifySelection"],
        (SHIFT, "End") => &["moveToEndOfDocumentAndModifySelection"],
        (SHIFT, "PageUp") => &["pageUpAndModifySelection"],
        (SHIFT, "PageDown") => &["pageDownAndModifySelection"],
        (CONTROL, "KeyA") => &["moveToBeginningOfParagraph"],
        (CONTROL, "KeyE") => &["moveToEndOfParagraph"],
        (CONTROL, "KeyB") => &["moveBackward"],
        (CONTROL, "KeyF") => &["moveForward"],
        (CONTROL, "KeyD") => &["deleteForward"],
        (CONTROL, "KeyH") => &["deleteBackward"],
        (CONTROL, "KeyK") => &["deleteToEndOfParagraph"],
        (CONTROL, "KeyN") => &["moveDown"],
        (CONTROL, "KeyP") => &["moveUp"],
        (CONTROL, "KeyV") => &["pageDown"],
        (CONTROL, "KeyY") => &["yank"],
        (CONTROL, "ArrowUp") => &["scrollPageUp"],
        (CONTROL, "ArrowDown") => &["scrollPageDown"],
        (CONTROL, "ArrowLeft") => &["moveToLeftEndOfLine"],
        (CONTROL, "ArrowRight") => &["moveToRightEndOfLine"],
        (CONTROL_SHIFT, "ArrowLeft") => &["moveToLeftEndOfLineAndModifySelection"],
        (CONTROL_SHIFT, "ArrowRight") => &["moveToRightEndOfLineAndModifySelection"],
        _ => &[],
    }
}

async fn viewport_size(page: &Page, expression: &str, fallback: f64) -> f64 {
    page.evaluate(expression)
        .await
        .ok()
        .and_then(|result| result.value().and_then(Value::as_f64))
        .filter(|value| *value > 0.0)
        .unwrap_or(fallback)
}

fn remote_text(object: &RemoteObject) -> String {
    if let Some(value) = &object.value {
        return match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
    }
    if let Some(value) = &object.unserializable_value {
        return value.as_ref().to_owned();
    }
    object.description.clone().unwrap_or_default()
}

fn push_console(logs: &Arc<Mutex<BrowserLogs>>, level: &str, text: &str) {
    let mut buffer = lock_logs(logs);
    buffer.push_console(level, text);
}

fn lock_logs(logs: &Arc<Mutex<BrowserLogs>>) -> std::sync::MutexGuard<'_, BrowserLogs> {
    logs.lock().unwrap_or_else(|poison| poison.into_inner())
}

async fn wait_ready(page: &Page) {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        let ready = page
            .evaluate("document.readyState === 'complete'")
            .await
            .ok()
            .and_then(|result| result.value().and_then(|value| value.as_bool()))
            .unwrap_or(false);
        if ready {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(READY_POLL).await;
    }
}

async fn page_info(page: &Page) -> Result<PageInfo, String> {
    let url = page
        .url()
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let title = page
        .get_title()
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let history = page
        .execute(GetNavigationHistoryParams::default())
        .await
        .map_err(|error| error.to_string())?
        .result;
    let can_go_back = history.current_index > 0;
    let can_go_forward = history.current_index + 1 < history.entries.len() as i64;
    Ok(PageInfo {
        url,
        title,
        can_go_back,
        can_go_forward,
    })
}

/// Finds a Chrome or Chromium binary on this computer.
pub fn default_chromium_path() -> Option<PathBuf> {
    chromiumoxide::detection::default_executable(Default::default())
        .ok()
        .filter(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_letters_to_their_key_codes() {
        let lower = key_definition("a", 0).expect("letter");
        assert_eq!(lower.key, "a");
        assert_eq!(lower.code, "KeyA");
        assert_eq!(lower.key_code, 65);
        assert_eq!(lower.text.as_deref(), Some("a"));

        let upper = key_definition("Z", 0).expect("letter");
        assert_eq!(upper.key, "z");
        assert_eq!(upper.code, "KeyZ");
        assert_eq!(upper.key_code, 90);
        assert_eq!(upper.text.as_deref(), Some("z"));
    }

    #[test]
    fn maps_digits_to_their_key_codes() {
        let zero = key_definition("0", 0).expect("digit");
        assert_eq!(zero.key, "0");
        assert_eq!(zero.code, "Digit0");
        assert_eq!(zero.key_code, 48);
        assert_eq!(zero.text.as_deref(), Some("0"));

        let nine = key_definition("9", 0).expect("digit");
        assert_eq!(nine.key, "9");
        assert_eq!(nine.code, "Digit9");
        assert_eq!(nine.key_code, 57);
        assert_eq!(nine.text.as_deref(), Some("9"));
    }

    #[test]
    fn shift_changes_the_letter_and_digit_values() {
        let letter = key_definition("a", MOD_SHIFT).expect("shifted letter");
        assert_eq!(letter.key, "A");
        assert_eq!(letter.text.as_deref(), Some("A"));

        let digit = key_definition("1", MOD_SHIFT).expect("shifted digit");
        assert_eq!(digit.key, "!");
        assert_eq!(digit.code, "Digit1");
        assert_eq!(digit.text.as_deref(), Some("!"));
    }

    #[test]
    fn every_command_modifier_suppresses_text() {
        for modifiers in [
            MOD_ALT,
            MOD_CONTROL,
            MOD_META,
            MOD_ALT | MOD_CONTROL,
            MOD_CONTROL | MOD_META,
            MOD_ALT | MOD_CONTROL | MOD_META,
        ] {
            let letter = key_definition("a", modifiers).expect("letter");
            assert_eq!(letter.text, None, "mask {modifiers}");
            assert_eq!(letter.key, "a", "mask {modifiers}");

            let named = key_definition("Enter", modifiers).expect("named key");
            assert_eq!(named.text, None, "mask {modifiers}");
            assert_eq!(named.key, "Enter", "mask {modifiers}");
        }
    }

    #[test]
    fn shift_and_a_command_modifier_keep_the_shifted_value() {
        let chord = key_definition("a", MOD_CONTROL | MOD_SHIFT).expect("chord");
        assert_eq!(chord.key, "A");
        assert_eq!(chord.text, None);
    }

    #[test]
    fn rejects_unsupported_keys() {
        assert!(key_definition("F13", 0).is_none());
        assert!(key_definition("!", 0).is_none());
        assert!(key_definition("ab", 0).is_none());
    }

    #[test]
    fn maps_the_common_mac_editing_shortcuts() {
        assert_eq!(editing_commands("KeyA", MOD_META), &["selectAll"]);
        assert_eq!(editing_commands("KeyC", MOD_META), &["copy"]);
        assert_eq!(editing_commands("KeyX", MOD_META), &["cut"]);
        assert_eq!(editing_commands("KeyV", MOD_META), &["paste"]);
        assert_eq!(editing_commands("KeyZ", MOD_META), &["undo"]);
        assert_eq!(editing_commands("KeyZ", MOD_META | MOD_SHIFT), &["redo"]);
        assert_eq!(
            editing_commands("Backspace", MOD_ALT),
            &["deleteWordBackward"]
        );
        assert_eq!(
            editing_commands("ArrowLeft", MOD_META),
            &["moveToLeftEndOfLine"]
        );
        assert_eq!(
            editing_commands("KeyA", MOD_CONTROL),
            &["moveToBeginningOfParagraph"]
        );
    }

    #[test]
    fn leaves_other_shortcuts_with_the_page() {
        assert_eq!(editing_commands("KeyA", 0), &[] as &[&str]);
        assert_eq!(editing_commands("KeyA", MOD_SHIFT), &[] as &[&str]);
        assert_eq!(editing_commands("KeyF", MOD_META), &[] as &[&str]);
        assert_eq!(editing_commands("Digit1", MOD_META), &[] as &[&str]);
    }
}
