//! Browser automation tools (Phase YY) — drive a headless Chrome /
//! Chromium over the DevTools Protocol via `chromiumoxide`.
//!
//! One lazily-launched browser session backs every tool, so the model can
//! `browser_navigate` → `browser_read` → `browser_click` →
//! `browser_screenshot` against the *same* page across calls. Requires a
//! Chrome / Chromium binary installed at runtime; if none is found, the
//! tools return a clear error rather than panicking.

use std::path::PathBuf;
use std::sync::Arc;

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use chromiumoxide::page::Page;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

/// Cap the text returned by `browser_read` so a huge page can't blow the
/// context window.
const MAX_TEXT: usize = 12_000;

/// A lazily-launched, shared headless browser. Cloneable — every clone
/// shares one browser process and one page.
#[derive(Clone, Default)]
pub struct BrowserSession {
    inner: Arc<Mutex<Option<Live>>>,
}

struct Live {
    // Kept alive for the lifetime of the session; dropping it closes Chrome.
    _browser: Browser,
    page: Page,
    _handler: tokio::task::JoinHandle<()>,
}

impl BrowserSession {
    /// A fresh, un-launched session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the shared page, launching Chrome on first use.
    async fn page(&self) -> std::result::Result<Page, String> {
        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            let config = BrowserConfig::builder()
                .build()
                .map_err(|e| format!("browser config: {e}"))?;
            let (browser, mut handler) = Browser::launch(config)
                .await
                .map_err(|e| format!("launch chrome: {e} (is Chrome/Chromium installed?)"))?;
            let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });
            let page = browser
                .new_page("about:blank")
                .await
                .map_err(|e| format!("open page: {e}"))?;
            *guard = Some(Live {
                _browser: browser,
                page,
                _handler: handler_task,
            });
        }
        // `Page` is a cheap handle (Arc inside) — clone it out so we don't
        // hold the session lock across the actual tool work.
        Ok(guard.as_ref().expect("just set").page.clone())
    }
}

/// Build the browser toolset, all sharing one [`BrowserSession`].
pub fn browser_tools() -> Vec<Arc<dyn ToolHandler>> {
    let s = BrowserSession::new();
    vec![
        Arc::new(BrowserNavigate { s: s.clone() }),
        Arc::new(BrowserRead { s: s.clone() }),
        Arc::new(BrowserClick { s: s.clone() }),
        Arc::new(BrowserScreenshot { s }),
    ]
}

fn tool_err(e: impl std::fmt::Display) -> AonyxError {
    AonyxError::Tool(e.to_string())
}

async fn title_and_url(page: &Page) -> (String, String) {
    let title = page.get_title().await.ok().flatten().unwrap_or_default();
    let url = page.url().await.ok().flatten().unwrap_or_default();
    (title, url)
}

/// `browser_navigate` — open a URL in the shared headless browser.
struct BrowserNavigate {
    s: BrowserSession,
}

#[derive(Deserialize)]
struct NavArgs {
    url: String,
}

#[async_trait]
impl ToolHandler for BrowserNavigate {
    fn name(&self) -> &str {
        "browser_navigate"
    }
    fn classify(&self) -> SafetyClass {
        SafetyClass::Caution
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Open a URL in the shared headless browser, then read it with browser_read.",
            "properties": { "url": { "type": "string", "description": "Absolute URL (http/https)." } },
            "required": ["url"]
        })
    }
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: NavArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("browser_navigate args: {e}")))?;
        let page = self.s.page().await.map_err(tool_err)?;
        page.goto(&args.url).await.map_err(tool_err)?;
        let _ = page.wait_for_navigation().await;
        let (title, url) = title_and_url(&page).await;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "url": url, "title": title }),
            error: None,
        })
    }
}

/// `browser_read` — return the current page's visible text.
struct BrowserRead {
    s: BrowserSession,
}

#[async_trait]
impl ToolHandler for BrowserRead {
    fn name(&self) -> &str {
        "browser_read"
    }
    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Read the visible text of the current browser page (navigate first).",
            "properties": {}
        })
    }
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let page = self.s.page().await.map_err(tool_err)?;
        let text: String = page
            .evaluate("document.body ? document.body.innerText : ''")
            .await
            .map_err(tool_err)?
            .into_value()
            .unwrap_or_default();
        let truncated = text.chars().count() > MAX_TEXT;
        let body: String = text.chars().take(MAX_TEXT).collect();
        let (title, url) = title_and_url(&page).await;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "url": url, "title": title, "text": body, "truncated": truncated }),
            error: None,
        })
    }
}

/// `browser_click` — click the first element matching a CSS selector.
struct BrowserClick {
    s: BrowserSession,
}

#[derive(Deserialize)]
struct ClickArgs {
    selector: String,
}

#[async_trait]
impl ToolHandler for BrowserClick {
    fn name(&self) -> &str {
        "browser_click"
    }
    fn classify(&self) -> SafetyClass {
        SafetyClass::Caution
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Click the first element matching a CSS selector on the current page.",
            "properties": { "selector": { "type": "string", "description": "A CSS selector, e.g. `a.login` or `button[type=submit]`." } },
            "required": ["selector"]
        })
    }
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: ClickArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("browser_click args: {e}")))?;
        let page = self.s.page().await.map_err(tool_err)?;
        let el = page
            .find_element(&args.selector)
            .await
            .map_err(|e| AonyxError::Tool(format!("selector '{}': {e}", args.selector)))?;
        el.click().await.map_err(tool_err)?;
        let _ = page.wait_for_navigation().await;
        let (title, url) = title_and_url(&page).await;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "clicked": args.selector, "url": url, "title": title }),
            error: None,
        })
    }
}

/// `browser_screenshot` — capture the current page as a PNG file.
struct BrowserScreenshot {
    s: BrowserSession,
}

#[derive(Deserialize)]
struct ShotArgs {
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl ToolHandler for BrowserScreenshot {
    fn name(&self) -> &str {
        "browser_screenshot"
    }
    fn classify(&self) -> SafetyClass {
        SafetyClass::Caution
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Save a PNG screenshot of the current page; returns the file path.",
            "properties": { "path": { "type": "string", "description": "Output path (default ./aonyx-screenshot.png)." } }
        })
    }
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: ShotArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("browser_screenshot args: {e}")))?;
        let page = self.s.page().await.map_err(tool_err)?;
        let params = chromiumoxide::page::ScreenshotParams::builder().build();
        let bytes = page.screenshot(params).await.map_err(tool_err)?;
        let path = PathBuf::from(
            args.path
                .unwrap_or_else(|| "aonyx-screenshot.png".to_string()),
        );
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| AonyxError::Tool(format!("write {}: {e}", path.display())))?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "path": path.display().to_string(), "bytes": bytes.len() }),
            error: None,
        })
    }
}
