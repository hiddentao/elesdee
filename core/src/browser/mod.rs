//! The session browser.
//!
//! Each session gets its own Chromium process and its own profile folder. The
//! profile holds the tabs' cookies, storage and logins, so two sessions never
//! share a login. The process stops when the session pauses, closes or the app
//! exits, while the profile stays on disk for the next start.

mod chromium;
mod stub;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;

pub const MAX_URL_CHARS: usize = 4096;
pub const MAX_SELECTOR_CHARS: usize = 500;
pub const MAX_TYPE_CHARS: usize = 4000;
pub const MAX_TEXT_CHARS: usize = 20_000;
pub const MAX_TABS: usize = 12;
pub const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_WAIT: Duration = Duration::from_secs(15);

/// One page in a session's browser.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// The browser state of one session. It is safe to read while an action runs.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub session_id: String,
    pub started: bool,
    pub available: bool,
    pub tabs: Vec<BrowserTab>,
    pub active_tab_id: Option<String>,
    /// Changes each time a new screenshot is captured. The UI uses it to
    /// refresh the live image.
    pub screenshot_seq: u64,
    pub error: Option<String>,
}

impl BrowserState {
    fn empty(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_owned(),
            started: false,
            available: true,
            tabs: Vec::new(),
            active_tab_id: None,
            screenshot_seq: 0,
            error: None,
        }
    }
}

#[derive(Debug)]
pub enum BrowserError {
    Unavailable(String),
    InvalidUrl(String),
    NoPage,
    TabLimit,
    Engine(String),
}

impl BrowserError {
    pub fn message(&self) -> String {
        match self {
            Self::Unavailable(reason) => format!(
                "The browser is not available. {reason} Set the CHROME environment variable to a Chrome or Chromium binary."
            ),
            Self::InvalidUrl(reason) => reason.clone(),
            Self::NoPage => "No page is open in this session's browser.".to_owned(),
            Self::TabLimit => format!("A session browser can have at most {MAX_TABS} tabs."),
            Self::Engine(reason) => format!("The browser action failed. {reason}"),
        }
    }
}

/// Finds the Chromium binary the core will run.
pub fn chromium_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CHROME").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    chromium::default_chromium_path()
}

#[derive(Clone)]
pub struct BrowserManager {
    inner: Arc<Inner>,
}

struct Inner {
    profile_root: Option<PathBuf>,
    /// Tests use a small stand-in instead of launching Chromium.
    stub: bool,
    contexts: Mutex<HashMap<String, Arc<Context>>>,
}

struct Context {
    session_id: String,
    profile: PathBuf,
    state: Mutex<Meta>,
    ops: AsyncMutex<()>,
}

struct Meta {
    available: bool,
    error: Option<String>,
    tabs: Vec<BrowserTab>,
    active: Option<String>,
    next_tab: u64,
    screenshot_seq: u64,
    screenshots: HashMap<String, Vec<u8>>,
    runtime: Option<Runtime>,
}

impl Meta {
    fn new() -> Self {
        Self {
            available: true,
            error: None,
            tabs: Vec::new(),
            active: None,
            next_tab: 1,
            screenshot_seq: 0,
            screenshots: HashMap::new(),
            runtime: None,
        }
    }

    fn engine(&self) -> Option<Engine> {
        self.runtime.as_ref().map(|runtime| runtime.engine.clone())
    }

    fn page(&self, tab_id: &str) -> Option<PageRef> {
        self.runtime
            .as_ref()
            .and_then(|runtime| runtime.pages.get(tab_id))
            .cloned()
    }
}

struct Runtime {
    engine: Engine,
    pages: HashMap<String, PageRef>,
}

#[derive(Clone)]
enum Engine {
    Chromium(chromium::Chromium),
    Stub(stub::Stub),
}

#[derive(Clone)]
enum PageRef {
    Chromium(chromiumoxide::Page),
    Stub(String),
}

struct PageInfo {
    url: String,
    title: String,
    can_go_back: bool,
    can_go_forward: bool,
}

impl BrowserManager {
    /// `profile_root` is the folder that holds one profile per session. Tests
    /// pass `None` for a temporary profile. `stub` selects the test engine.
    pub fn new(profile_root: Option<PathBuf>, stub: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                profile_root,
                stub,
                contexts: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn context(&self, session_id: &str) -> Arc<Context> {
        let mut contexts = self
            .inner
            .contexts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(context) = contexts.get(session_id) {
            return Arc::clone(context);
        }
        let profile = self
            .inner
            .profile_root
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("elesdee-browsers"))
            .join(profile_name(session_id));
        let context = Arc::new(Context {
            session_id: session_id.to_owned(),
            profile,
            state: Mutex::new(Meta::new()),
            ops: AsyncMutex::new(()),
        });
        contexts.insert(session_id.to_owned(), Arc::clone(&context));
        context
    }

    fn existing(&self, session_id: &str) -> Option<Arc<Context>> {
        self.inner
            .contexts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(session_id)
            .cloned()
    }

    pub fn state(&self, session_id: &str) -> BrowserState {
        let Some(context) = self.existing(session_id) else {
            return BrowserState::empty(session_id);
        };
        let meta = lock_meta(&context);
        BrowserState {
            session_id: session_id.to_owned(),
            started: meta.runtime.is_some(),
            available: meta.available,
            tabs: meta.tabs.clone(),
            active_tab_id: meta.active.clone(),
            screenshot_seq: meta.screenshot_seq,
            error: meta.error.clone(),
        }
    }

    /// Starts the session's browser if it is not running. Chromium starts on
    /// the first action, not when the session starts.
    pub async fn start(&self, session_id: &str) -> Result<BrowserState, BrowserError> {
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        Ok(self.state(session_id))
    }

    async fn ensure_runtime(&self, context: &Context) -> Result<(), BrowserError> {
        {
            let meta = lock_meta(context);
            if meta.runtime.is_some() {
                return Ok(());
            }
            if !meta.available {
                return Err(BrowserError::Unavailable(
                    "No Chrome or Chromium binary was found.".to_owned(),
                ));
            }
        }
        let engine = if self.inner.stub {
            Engine::Stub(stub::Stub::new())
        } else {
            match chromium::Chromium::launch(&context.profile).await {
                Ok(chromium) => Engine::Chromium(chromium),
                Err(error) => {
                    let mut meta = lock_meta(context);
                    meta.available = false;
                    meta.error = Some(error.message());
                    return Err(error);
                }
            }
        };
        let mut meta = lock_meta(context);
        meta.runtime = Some(Runtime {
            engine,
            pages: HashMap::new(),
        });
        meta.available = true;
        meta.error = None;
        Ok(())
    }

    /// Stops the browser process and closes its tabs. The profile stays on
    /// disk, so the session keeps its logins.
    pub async fn stop(&self, session_id: &str) {
        let Some(context) = self.existing(session_id) else {
            return;
        };
        let _ops = context.ops.lock().await;
        let runtime = lock_meta(&context).runtime.take();
        if let Some(runtime) = runtime {
            runtime.engine.stop().await;
        }
        let mut meta = lock_meta(&context);
        meta.tabs.clear();
        meta.active = None;
        meta.screenshots.clear();
        meta.error = None;
    }

    /// Stops every browser. Used when the core shuts down.
    pub async fn stop_all(&self) {
        let contexts = self
            .inner
            .contexts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for context in contexts {
            self.stop(&context.session_id).await;
        }
    }

    /// Opens a URL in the active tab, or in a new tab when none is open.
    pub async fn open(&self, session_id: &str, url: &str) -> Result<BrowserState, BrowserError> {
        let url = checked_url(url)?;
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = self.ensure_active_tab(&context).await?;
        mark_loading(&context, &tab_id, true);
        let (engine, page) = action_target(&context, &tab_id)?;
        let result = engine.goto(page.as_ref(), &url).await;
        finish_action(&context, &engine, page.as_ref(), &tab_id, result).await
    }

    pub async fn new_tab(
        &self,
        session_id: &str,
        url: Option<&str>,
    ) -> Result<BrowserState, BrowserError> {
        let url = match url {
            Some(value) => Some(checked_url(value)?),
            None => None,
        };
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = self.create_tab(&context).await?;
        match url {
            Some(url) => {
                mark_loading(&context, &tab_id, true);
                let (engine, page) = action_target(&context, &tab_id)?;
                let result = engine.goto(page.as_ref(), &url).await;
                finish_action(&context, &engine, page.as_ref(), &tab_id, result).await
            }
            None => Ok(self.state(session_id)),
        }
    }

    pub async fn activate(
        &self,
        session_id: &str,
        tab_id: &str,
    ) -> Result<BrowserState, BrowserError> {
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        {
            let mut meta = lock_meta(&context);
            if !meta.tabs.iter().any(|tab| tab.id == tab_id) {
                return Err(BrowserError::NoPage);
            }
            meta.active = Some(tab_id.to_owned());
        }
        let (engine, page) = action_target(&context, tab_id)?;
        capture(&context, &engine, page.as_ref(), tab_id).await;
        Ok(self.state(session_id))
    }

    pub async fn close_tab(
        &self,
        session_id: &str,
        tab_id: &str,
    ) -> Result<BrowserState, BrowserError> {
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let removed = {
            let mut meta = lock_meta(&context);
            let removed = meta
                .runtime
                .as_mut()
                .and_then(|runtime| runtime.pages.remove(tab_id));
            if removed.is_some() {
                meta.tabs.retain(|tab| tab.id != tab_id);
                meta.screenshots.remove(tab_id);
                if meta.active.as_deref() == Some(tab_id) {
                    meta.active = meta.tabs.last().map(|tab| tab.id.clone());
                }
            }
            removed
        };
        let Some(page) = removed else {
            return Err(BrowserError::NoPage);
        };
        let engine = {
            let meta = lock_meta(&context);
            meta.engine()
        };
        if let Some(engine) = engine {
            engine.close_page(page).await;
        }
        Ok(self.state(session_id))
    }

    pub async fn back(&self, session_id: &str) -> Result<BrowserState, BrowserError> {
        self.history(session_id, true).await
    }

    pub async fn forward(&self, session_id: &str) -> Result<BrowserState, BrowserError> {
        self.history(session_id, false).await
    }

    async fn history(&self, session_id: &str, back: bool) -> Result<BrowserState, BrowserError> {
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = active_tab(&context).ok_or(BrowserError::NoPage)?;
        mark_loading(&context, &tab_id, true);
        let (engine, page) = action_target(&context, &tab_id)?;
        let result = if back {
            engine.back(page.as_ref()).await
        } else {
            engine.forward(page.as_ref()).await
        };
        finish_action(&context, &engine, page.as_ref(), &tab_id, result).await
    }

    pub async fn reload(&self, session_id: &str) -> Result<BrowserState, BrowserError> {
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = active_tab(&context).ok_or(BrowserError::NoPage)?;
        mark_loading(&context, &tab_id, true);
        let (engine, page) = action_target(&context, &tab_id)?;
        let result = engine.reload(page.as_ref()).await;
        finish_action(&context, &engine, page.as_ref(), &tab_id, result).await
    }

    /// Returns the visible text of the active page.
    pub async fn read_text(&self, session_id: &str) -> Result<String, BrowserError> {
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = active_tab(&context).ok_or(BrowserError::NoPage)?;
        let (engine, page) = action_target(&context, &tab_id)?;
        let text = engine
            .text(page.as_ref())
            .await
            .map_err(BrowserError::Engine)?;
        Ok(cap_text(&text))
    }

    pub async fn click(
        &self,
        session_id: &str,
        selector: &str,
    ) -> Result<BrowserState, BrowserError> {
        let selector = checked_selector(selector)?;
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = active_tab(&context).ok_or(BrowserError::NoPage)?;
        let (engine, page) = action_target(&context, &tab_id)?;
        let result = engine.click(page.as_ref(), &selector).await;
        finish_action(&context, &engine, page.as_ref(), &tab_id, result).await
    }

    pub async fn type_text(
        &self,
        session_id: &str,
        selector: &str,
        text: &str,
    ) -> Result<BrowserState, BrowserError> {
        let selector = checked_selector(selector)?;
        if text.chars().count() > MAX_TYPE_CHARS {
            return Err(BrowserError::Engine(format!(
                "The text is longer than {MAX_TYPE_CHARS} characters."
            )));
        }
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = active_tab(&context).ok_or(BrowserError::NoPage)?;
        let (engine, page) = action_target(&context, &tab_id)?;
        let result = engine.type_text(page.as_ref(), &selector, text).await;
        finish_action(&context, &engine, page.as_ref(), &tab_id, result).await
    }

    /// Waits for a selector, a text, or the timeout.
    pub async fn wait(
        &self,
        session_id: &str,
        selector: Option<&str>,
        text: Option<&str>,
        timeout: Duration,
    ) -> Result<BrowserState, BrowserError> {
        let selector = match selector {
            Some(value) => Some(checked_selector(value)?),
            None => None,
        };
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = active_tab(&context).ok_or(BrowserError::NoPage)?;
        let (engine, page) = action_target(&context, &tab_id)?;
        engine
            .wait(page.as_ref(), selector.as_deref(), text, timeout)
            .await
            .map_err(BrowserError::Engine)?;
        capture(&context, &engine, page.as_ref(), &tab_id).await;
        Ok(self.state(session_id))
    }

    /// Captures the active page and returns the PNG bytes.
    pub async fn screenshot(
        &self,
        session_id: &str,
    ) -> Result<(Vec<u8>, BrowserState), BrowserError> {
        let context = self.context(session_id);
        let _ops = context.ops.lock().await;
        self.ensure_runtime(&context).await?;
        let tab_id = active_tab(&context).ok_or(BrowserError::NoPage)?;
        let (engine, page) = action_target(&context, &tab_id)?;
        let bytes = engine
            .screenshot(page.as_ref())
            .await
            .map_err(BrowserError::Engine)?;
        if bytes.len() > MAX_SCREENSHOT_BYTES {
            return Err(BrowserError::Engine(
                "The screenshot is too large.".to_owned(),
            ));
        }
        {
            let mut meta = lock_meta(&context);
            meta.screenshots.insert(tab_id, bytes.clone());
            meta.screenshot_seq += 1;
        }
        Ok((bytes, self.state(session_id)))
    }

    /// Returns the latest captured PNG for a tab, if one exists.
    pub fn screenshot_bytes(&self, session_id: &str, tab_id: &str) -> Option<Vec<u8>> {
        let context = self.existing(session_id)?;
        lock_meta(&context).screenshots.get(tab_id).cloned()
    }

    async fn create_tab(&self, context: &Context) -> Result<String, BrowserError> {
        let (engine, tab_id) = {
            let mut meta = lock_meta(context);
            if meta
                .runtime
                .as_ref()
                .is_none_or(|runtime| runtime.pages.len() >= MAX_TABS)
            {
                return Err(BrowserError::TabLimit);
            }
            let engine = meta
                .engine()
                .ok_or_else(|| BrowserError::Engine("The browser did not start.".to_owned()))?;
            let tab_id = format!(
                "{}-tab-{}",
                profile_name(&context.session_id),
                meta.next_tab
            );
            meta.next_tab += 1;
            (engine, tab_id)
        };
        let page = engine.new_page().await.map_err(BrowserError::Engine)?;
        let mut meta = lock_meta(context);
        if let Some(runtime) = meta.runtime.as_mut() {
            runtime.pages.insert(tab_id.clone(), page);
        }
        meta.tabs.push(BrowserTab {
            id: tab_id.clone(),
            url: String::new(),
            title: String::new(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
        });
        meta.active = Some(tab_id.clone());
        Ok(tab_id)
    }

    async fn ensure_active_tab(&self, context: &Context) -> Result<String, BrowserError> {
        if let Some(tab) = active_tab(context) {
            return Ok(tab);
        }
        self.create_tab(context).await
    }
}

fn profile_name(session_id: &str) -> String {
    let safe = session_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if safe.is_empty() {
        "session".to_owned()
    } else {
        safe
    }
}

fn lock_meta(context: &Context) -> std::sync::MutexGuard<'_, Meta> {
    context
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn active_tab(context: &Context) -> Option<String> {
    lock_meta(context).active.clone()
}

fn action_target(
    context: &Context,
    tab_id: &str,
) -> Result<(Engine, Option<PageRef>), BrowserError> {
    let meta = lock_meta(context);
    let engine = meta
        .engine()
        .ok_or_else(|| BrowserError::Engine("The browser is not running.".to_owned()))?;
    Ok((engine, meta.page(tab_id)))
}

fn mark_loading(context: &Context, tab_id: &str, loading: bool) {
    let mut meta = lock_meta(context);
    if let Some(tab) = meta.tabs.iter_mut().find(|tab| tab.id == tab_id) {
        tab.loading = loading;
    }
}

async fn finish_action(
    context: &Context,
    engine: &Engine,
    page: Option<&PageRef>,
    tab_id: &str,
    result: Result<PageInfo, String>,
) -> Result<BrowserState, BrowserError> {
    match result {
        Ok(info) => {
            {
                let mut meta = lock_meta(context);
                if let Some(tab) = meta.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    tab.url = info.url;
                    tab.title = info.title;
                    tab.loading = false;
                    tab.can_go_back = info.can_go_back;
                    tab.can_go_forward = info.can_go_forward;
                }
            }
            capture(context, engine, page, tab_id).await;
            let meta = lock_meta(context);
            Ok(BrowserState {
                session_id: context.session_id.clone(),
                started: meta.runtime.is_some(),
                available: meta.available,
                tabs: meta.tabs.clone(),
                active_tab_id: meta.active.clone(),
                screenshot_seq: meta.screenshot_seq,
                error: meta.error.clone(),
            })
        }
        Err(error) => {
            mark_loading(context, tab_id, false);
            Err(BrowserError::Engine(error))
        }
    }
}

async fn capture(context: &Context, engine: &Engine, page: Option<&PageRef>, tab_id: &str) {
    match engine.screenshot(page).await {
        Ok(bytes) if bytes.len() <= MAX_SCREENSHOT_BYTES => {
            let mut meta = lock_meta(context);
            meta.screenshots.insert(tab_id.to_owned(), bytes);
            meta.screenshot_seq += 1;
        }
        _ => {}
    }
}

fn cap_text(text: &str) -> String {
    let clean = text.trim();
    if clean.chars().count() <= MAX_TEXT_CHARS {
        return clean.to_owned();
    }
    let mut capped = clean.chars().take(MAX_TEXT_CHARS).collect::<String>();
    capped.push_str("\n[The page text is longer and was shortened.]");
    capped
}

fn checked_url(value: &str) -> Result<String, BrowserError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(BrowserError::InvalidUrl(
            "The address is required.".to_owned(),
        ));
    }
    if value.chars().count() > MAX_URL_CHARS {
        return Err(BrowserError::InvalidUrl(
            "The address is too long.".to_owned(),
        ));
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| {
        BrowserError::InvalidUrl("The address must be a full http or https URL.".to_owned())
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(BrowserError::InvalidUrl(
                "Only http and https addresses can be opened.".to_owned(),
            ));
        }
    }
    if parsed.host().is_none() {
        return Err(BrowserError::InvalidUrl(
            "The address needs a host.".to_owned(),
        ));
    }
    Ok(parsed.to_string())
}

fn checked_selector(value: &str) -> Result<String, BrowserError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(BrowserError::Engine("The selector is required.".to_owned()));
    }
    if value.chars().count() > MAX_SELECTOR_CHARS {
        return Err(BrowserError::Engine("The selector is too long.".to_owned()));
    }
    Ok(value.to_owned())
}

impl Engine {
    async fn new_page(&self) -> Result<PageRef, String> {
        match self {
            Engine::Chromium(chromium) => Ok(PageRef::Chromium(chromium.new_page().await?)),
            Engine::Stub(stub) => Ok(PageRef::Stub(stub.new_page())),
        }
    }

    async fn close_page(&self, page: PageRef) {
        if let (Engine::Chromium(_), PageRef::Chromium(page)) = (self, page) {
            let _ = page.close().await;
        }
    }

    async fn goto(&self, page: Option<&PageRef>, url: &str) -> Result<PageInfo, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.goto(page, url).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => stub.goto(page, url).await,
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn back(&self, page: Option<&PageRef>) -> Result<PageInfo, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.back(page).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => stub.back(page).await,
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn forward(&self, page: Option<&PageRef>) -> Result<PageInfo, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.forward(page).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => stub.forward(page).await,
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn reload(&self, page: Option<&PageRef>) -> Result<PageInfo, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.reload(page).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => stub.reload(page).await,
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn text(&self, page: Option<&PageRef>) -> Result<String, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.text(page).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => Ok(stub.text(page)),
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn click(&self, page: Option<&PageRef>, selector: &str) -> Result<PageInfo, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.click(page, selector).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => stub.click(page, selector).await,
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn type_text(
        &self,
        page: Option<&PageRef>,
        selector: &str,
        text: &str,
    ) -> Result<PageInfo, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.type_text(page, selector, text).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => {
                stub.type_text(page, selector, text).await
            }
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn wait(
        &self,
        page: Option<&PageRef>,
        selector: Option<&str>,
        text: Option<&str>,
        timeout: Duration,
    ) -> Result<(), String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.wait(page, selector, text, timeout).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(page))) => {
                stub.wait(page, selector, text, timeout).await
            }
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn screenshot(&self, page: Option<&PageRef>) -> Result<Vec<u8>, String> {
        match (self, page) {
            (Engine::Chromium(chromium), Some(PageRef::Chromium(page))) => {
                chromium.screenshot(page).await
            }
            (Engine::Stub(stub), Some(PageRef::Stub(_))) => Ok(stub.screenshot()),
            _ => Err("No page is open.".to_owned()),
        }
    }

    async fn stop(self) {
        if let Engine::Chromium(chromium) = self {
            chromium.stop().await;
        }
    }
}
