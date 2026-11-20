use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::PageInfo;

/// A small stand-in for the browser in core tests.
///
/// It fetches local pages over HTTP so the tools can read real text, and it
/// records clicks and typing without rendering. Production always uses
/// Chromium.
#[derive(Clone)]
pub struct Stub {
    inner: Arc<Mutex<Inner>>,
    client: reqwest::Client,
}

struct Inner {
    pages: HashMap<String, StubPage>,
    next: u64,
}

struct StubPage {
    url: String,
    title: String,
    text: String,
    history: Vec<String>,
    index: usize,
}

impl Stub {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                pages: HashMap::new(),
                next: 1,
            })),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn new_page(&self) -> String {
        let mut inner = lock(self);
        let id = format!("stub-page-{}", inner.next);
        inner.next += 1;
        inner.pages.insert(
            id.clone(),
            StubPage {
                url: "about:blank".to_owned(),
                title: String::new(),
                text: String::new(),
                history: vec!["about:blank".to_owned()],
                index: 0,
            },
        );
        id
    }

    pub async fn goto(&self, page: &str, url: &str) -> Result<PageInfo, String> {
        let (title, text) = self.fetch(url).await?;
        let mut inner = lock(self);
        let page = inner
            .pages
            .get_mut(page)
            .ok_or_else(|| "No page is open.".to_owned())?;
        page.history.truncate(page.index + 1);
        page.history.push(url.to_owned());
        page.index = page.history.len() - 1;
        page.url = url.to_owned();
        page.title = title;
        page.text = text;
        Ok(page_info(page))
    }

    pub async fn back(&self, page: &str) -> Result<PageInfo, String> {
        let mut inner = lock(self);
        let page = inner
            .pages
            .get_mut(page)
            .ok_or_else(|| "No page is open.".to_owned())?;
        if page.index > 0 {
            page.index -= 1;
            page.url = page.history[page.index].clone();
        }
        Ok(page_info(page))
    }

    pub async fn forward(&self, page: &str) -> Result<PageInfo, String> {
        let mut inner = lock(self);
        let page = inner
            .pages
            .get_mut(page)
            .ok_or_else(|| "No page is open.".to_owned())?;
        if page.index + 1 < page.history.len() {
            page.index += 1;
            page.url = page.history[page.index].clone();
        }
        Ok(page_info(page))
    }

    pub async fn reload(&self, page: &str) -> Result<PageInfo, String> {
        let url = {
            let inner = lock(self);
            inner
                .pages
                .get(page)
                .ok_or_else(|| "No page is open.".to_owned())?
                .url
                .clone()
        };
        self.goto(page, &url).await
    }

    pub fn text(&self, page: &str) -> String {
        let inner = lock(self);
        inner
            .pages
            .get(page)
            .map(|page| page.text.clone())
            .unwrap_or_default()
    }

    pub async fn click(&self, page: &str, _selector: &str) -> Result<PageInfo, String> {
        let inner = lock(self);
        inner
            .pages
            .get(page)
            .map(page_info)
            .ok_or_else(|| "No page is open.".to_owned())
    }

    pub async fn type_text(
        &self,
        page: &str,
        _selector: &str,
        text: &str,
    ) -> Result<PageInfo, String> {
        let mut inner = lock(self);
        let page = inner
            .pages
            .get_mut(page)
            .ok_or_else(|| "No page is open.".to_owned())?;
        page.text.push_str(text);
        Ok(page_info(page))
    }

    pub async fn wait(
        &self,
        _page: &str,
        _selector: Option<&str>,
        _text: Option<&str>,
        timeout: Duration,
    ) -> Result<(), String> {
        // The stand-in page is already loaded.
        tokio::time::sleep(timeout.min(Duration::from_millis(10))).await;
        Ok(())
    }

    pub fn screenshot(&self) -> Vec<u8> {
        static PNG: OnceLock<Vec<u8>> = OnceLock::new();
        PNG.get_or_init(|| {
            let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([32, 96, 176, 255]));
            let mut bytes = Vec::new();
            image
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .expect("encode the stub screenshot");
            bytes
        })
        .clone()
    }

    async fn fetch(&self, url: &str) -> Result<(String, String), String> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("The page could not be opened: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("The page returned status {}.", response.status()));
        }
        let html = response
            .text()
            .await
            .map_err(|error| format!("The page could not be read: {error}"))?;
        Ok((title_of(&html), visible_text(&html)))
    }
}

fn lock(stub: &Stub) -> std::sync::MutexGuard<'_, Inner> {
    stub.inner
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn page_info(page: &StubPage) -> PageInfo {
    PageInfo {
        url: page.url.clone(),
        title: page.title.clone(),
        can_go_back: page.index > 0,
        can_go_forward: page.index + 1 < page.history.len(),
    }
}

fn title_of(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let Some(start) = lower.find("<title") else {
        return String::new();
    };
    let Some(open_end) = lower[start..].find('>').map(|offset| start + offset + 1) else {
        return String::new();
    };
    let Some(end) = lower[open_end..]
        .find("</title>")
        .map(|offset| open_end + offset)
    else {
        return String::new();
    };
    decode_entities(html[open_end..end].trim())
}

fn visible_text(html: &str) -> String {
    let body = body_content(html);
    let mut output = String::with_capacity(body.len());
    let mut rest = body;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(open) = lower.find("<script").or_else(|| lower.find("<style")) else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..open]);
        let tag = if lower[open..].starts_with("<script") {
            "</script>"
        } else {
            "</style>"
        };
        match lower[open..].find(tag) {
            Some(end) => rest = &rest[open + end + tag.len()..],
            None => break,
        }
    }

    let mut text = String::with_capacity(output.len());
    let mut in_tag = false;
    for character in output.chars() {
        match character {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn body_content(html: &str) -> &str {
    let lower = html.to_ascii_lowercase();
    let Some(start) = lower.find("<body") else {
        return html;
    };
    let Some(open_end) = lower[start..].find('>').map(|offset| start + offset + 1) else {
        return html;
    };
    let end = lower[open_end..]
        .find("</body>")
        .map(|offset| open_end + offset)
        .unwrap_or(html.len());
    &html[open_end..end]
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_title_and_visible_text() {
        let html = "<html><head><title>A test</title><style>body{}</style></head>\
            <body><script>ignore()</script><h1>Hello</h1><p>World &amp; more</p></body></html>";
        assert_eq!(title_of(html), "A test");
        assert_eq!(visible_text(html), "Hello World & more");
    }
}
