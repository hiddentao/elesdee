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

    /// Presses a named key that does not insert text, such as Enter or an
    /// arrow key.
    pub async fn press_key(
        &self,
        page: &Page,
        key: &str,
        modifiers: u8,
    ) -> Result<PageInfo, String> {
        let definition = key_definition(key)
            .ok_or_else(|| format!("The live view cannot press the key {key}."))?;
        let down = if definition.text.is_some() {
            DispatchKeyEventType::KeyDown
        } else {
            DispatchKeyEventType::RawKeyDown
        };
        let mut pressed = DispatchKeyEventParams::new(down);
        pressed.modifiers = Some(i64::from(modifiers));
        pressed.key = Some(key.to_owned());
        pressed.code = Some(definition.code.to_owned());
        pressed.windows_virtual_key_code = Some(definition.key_code);
        pressed.native_virtual_key_code = Some(definition.key_code);
        if let Some(text) = definition.text {
            pressed.text = Some(text.to_owned());
            pressed.unmodified_text = Some(text.to_owned());
        }
        page.execute(pressed)
            .await
            .map_err(|error| error.to_string())?;

        let mut released = DispatchKeyEventParams::new(DispatchKeyEventType::KeyUp);
        released.modifiers = Some(i64::from(modifiers));
        released.key = Some(key.to_owned());
        released.code = Some(definition.code.to_owned());
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

struct KeyDefinition {
    code: &'static str,
    key_code: i64,
    text: Option<&'static str>,
}

/// The non-text keys that the live view can press.
fn key_definition(key: &str) -> Option<KeyDefinition> {
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
        code,
        key_code,
        text,
    })
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
