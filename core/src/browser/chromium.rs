use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, GetNavigationHistoryParams, NavigateToHistoryEntryParams,
};
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use tokio::task::JoinHandle;

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
    pub async fn launch(profile: &Path) -> Result<Self, BrowserError> {
        let executable = super::chromium_path().ok_or_else(|| {
            BrowserError::Unavailable("No Chrome or Chromium binary was found.".to_owned())
        })?;
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
