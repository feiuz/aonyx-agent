//! Web tools: `web_fetch` (HTML → text) and `web_search` (Brave).
//!
//! Both are [`SafetyClass::Safe`] — read-only network access. They use
//! the async `reqwest` client (rustls TLS, so no system OpenSSL).
//!
//! `web_search` calls the Brave Search API and reads its key from the
//! `BRAVE_API_KEY` environment variable; without it the tool returns a
//! helpful error rather than failing opaquely.

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

/// Hard cap on the text returned by `web_fetch` so a huge page can't
/// blow the model's context.
const FETCH_MAX_CHARS: usize = 20_000;
/// Default number of `web_search` results.
const SEARCH_DEFAULT_COUNT: u64 = 5;
/// Brave Search API endpoint.
const BRAVE_ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
/// Tavily Search API endpoint (fallback when Brave is unconfigured).
const TAVILY_ENDPOINT: &str = "https://api.tavily.com/search";

/// GET `url` and return `(content_type, body_bytes)` (Phase OO).
///
/// Shared raw-bytes fetch — the CLI reuses it to pull a remote image
/// URL into a vision attachment (rather than only local `@file` refs).
/// Read-only; the caller owns any size / format handling.
pub async fn fetch_bytes(url: &str) -> Result<(String, Vec<u8>)> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(AonyxError::Tool(format!(
            "fetch_bytes: url must be http(s): {url}"
        )));
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header(
            "user-agent",
            "aonyx-agent/0.2 (+https://github.com/feiuz/aonyx-agent)",
        )
        .send()
        .await
        .map_err(|e| AonyxError::Tool(format!("fetch_bytes {url}: {e}")))?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AonyxError::Tool(format!("fetch_bytes body: {e}")))?;
    Ok((content_type, bytes.to_vec()))
}

/// `web_fetch` — GET a URL and return its readable text. Safe.
pub struct WebFetch;

#[derive(Deserialize)]
struct WebFetchArgs {
    url: String,
}

#[async_trait]
impl ToolHandler for WebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Absolute http(s) URL to fetch." }
            },
            "required": ["url"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: WebFetchArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("web_fetch args: {e}")))?;
        if !args.url.starts_with("http://") && !args.url.starts_with("https://") {
            return Err(AonyxError::Tool(format!(
                "web_fetch: url must be http(s): {}",
                args.url
            )));
        }
        let client = reqwest::Client::new();
        let resp = client
            .get(&args.url)
            .header(
                "user-agent",
                "aonyx-agent/0.2 (+https://github.com/feiuz/aonyx-agent)",
            )
            .send()
            .await
            .map_err(|e| AonyxError::Tool(format!("web_fetch {}: {e}", args.url)))?;
        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp
            .text()
            .await
            .map_err(|e| AonyxError::Tool(format!("web_fetch body: {e}")))?;

        // HTML gets stripped to text; anything else (json, plain) is
        // returned as-is (still truncated).
        let text = if content_type.contains("html") || looks_like_html(&body) {
            html_to_text(&body)
        } else {
            body
        };
        let truncated = truncate_chars(&text, FETCH_MAX_CHARS);

        Ok(ToolResult {
            call_id: call.id,
            output: json!({
                "url": args.url,
                "status": status.as_u16(),
                "content_type": content_type,
                "text": truncated,
            }),
            error: None,
        })
    }
}

/// `web_search` — Brave Search API. Safe. Needs `BRAVE_API_KEY`.
pub struct WebSearch;

#[derive(Deserialize)]
struct WebSearchArgs {
    query: String,
    #[serde(default)]
    count: Option<u64>,
}

#[async_trait]
impl ToolHandler for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "count": { "type": "integer", "minimum": 1, "maximum": 20, "default": 5 }
            },
            "required": ["query"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: WebSearchArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("web_search args: {e}")))?;
        let count = args.count.unwrap_or(SEARCH_DEFAULT_COUNT).clamp(1, 20);
        let client = reqwest::Client::new();

        // Prefer Brave; fall back to Tavily (Phase MM). At least one key
        // must be set.
        let (provider, results) = if let Ok(key) = std::env::var("BRAVE_API_KEY") {
            let payload = brave_request(&client, &key, &args.query, count).await?;
            ("brave", parse_brave_results(&payload, count as usize))
        } else if let Ok(key) = std::env::var("TAVILY_API_KEY") {
            let payload = tavily_request(&client, &key, &args.query, count).await?;
            ("tavily", parse_tavily_results(&payload, count as usize))
        } else {
            return Err(AonyxError::Tool(
                "web_search: set BRAVE_API_KEY (brave.com/search/api) or TAVILY_API_KEY \
                 (tavily.com) to enable search"
                    .to_string(),
            ));
        };

        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "query": args.query, "provider": provider, "results": results }),
            error: None,
        })
    }
}

/// Issue a Brave web-search request and return the decoded JSON.
async fn brave_request(
    client: &reqwest::Client,
    key: &str,
    query: &str,
    count: u64,
) -> Result<Value> {
    let resp = client
        .get(BRAVE_ENDPOINT)
        .header("x-subscription-token", key)
        .header("accept", "application/json")
        .query(&[("q", query), ("count", &count.to_string())])
        .send()
        .await
        .map_err(|e| AonyxError::Tool(format!("web_search (brave) send: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(AonyxError::Tool(format!(
            "web_search (brave): HTTP {status}: {txt}"
        )));
    }
    resp.json()
        .await
        .map_err(|e| AonyxError::Tool(format!("web_search (brave) json: {e}")))
}

/// Issue a Tavily search request (POST) and return the decoded JSON.
async fn tavily_request(
    client: &reqwest::Client,
    key: &str,
    query: &str,
    count: u64,
) -> Result<Value> {
    let body = json!({
        "api_key": key,
        "query": query,
        "max_results": count,
        "search_depth": "basic",
    });
    let resp = client
        .post(TAVILY_ENDPOINT)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AonyxError::Tool(format!("web_search (tavily) send: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(AonyxError::Tool(format!(
            "web_search (tavily): HTTP {status}: {txt}"
        )));
    }
    resp.json()
        .await
        .map_err(|e| AonyxError::Tool(format!("web_search (tavily) json: {e}")))
}

// ---- pure helpers (unit-tested) ----

/// Heuristic: does this body look like HTML even without a content-type?
fn looks_like_html(body: &str) -> bool {
    let head = body.trim_start();
    let lower = head.get(..16).unwrap_or(head).to_lowercase();
    lower.starts_with("<!doctype") || lower.starts_with("<html")
}

/// Strip HTML tags to readable text: drops `<script>` / `<style>`
/// bodies, removes all tags, decodes a handful of common entities, and
/// collapses runs of whitespace. Not a full DOM parse — good enough to
/// feed a model the gist of a page.
pub fn html_to_text(html: &str) -> String {
    // Readability pass (Phase MM): focus on the main content and drop
    // page chrome before stripping tags.
    let main = isolate_main(html);
    let without_blocks = strip_blocks(&main, "script");
    let without_blocks = strip_blocks(&without_blocks, "style");
    let without_blocks = strip_blocks(&without_blocks, "nav");
    let without_blocks = strip_blocks(&without_blocks, "header");
    let without_blocks = strip_blocks(&without_blocks, "footer");
    let without_blocks = strip_blocks(&without_blocks, "aside");

    // Remove tags.
    let mut out = String::with_capacity(without_blocks.len());
    let mut in_tag = false;
    for c in without_blocks.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }

    let decoded = decode_entities(&out);
    collapse_whitespace(&decoded)
}

/// Readability heuristic (Phase MM): if the page has a `<main>` or
/// `<article>` element, return just that subtree's HTML; otherwise the
/// whole document. Prefers `<article>` (usually tighter than `<main>`).
fn isolate_main(html: &str) -> String {
    for tag in ["article", "main"] {
        if let Some(inner) = first_block_inner(html, tag) {
            if inner.trim().len() > 200 {
                return inner;
            }
        }
    }
    html.to_string()
}

/// Return the inner HTML of the first `<tag …> … </tag>` block
/// (case-insensitive), or `None` when absent / unclosed.
fn first_block_inner(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let open_at = lower.find(&open)?;
    // Skip to the end of the opening tag (`>`).
    let after_open = html[open_at..].find('>')? + open_at + 1;
    let close_rel = lower[after_open..].find(&close)?;
    Some(html[after_open..after_open + close_rel].to_string())
}

/// Remove `<tag …> … </tag>` blocks (case-insensitive) including their
/// contents. Used to drop `<script>`, `<style>`, and page chrome.
fn strip_blocks(html: &str, tag: &str) -> String {
    let lower = html.to_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0usize;
    while let Some(rel) = lower[cursor..].find(&open) {
        let start = cursor + rel;
        out.push_str(&html[cursor..start]);
        // Find the matching close tag; if absent, drop the rest.
        match lower[start..].find(&close) {
            Some(crel) => cursor = start + crel + close.len(),
            None => {
                cursor = html.len();
                break;
            }
        }
    }
    out.push_str(&html[cursor..]);
    out
}

/// Decode the handful of HTML entities that show up most in prose.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// Collapse runs of whitespace to single spaces, trim, and keep at most
/// one blank line between paragraphs.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;
    let mut newlines = 0u8;
    for c in s.chars() {
        if c == '\n' {
            newlines = newlines.saturating_add(1);
            last_was_space = true;
            continue;
        }
        if c.is_whitespace() {
            last_was_space = true;
            continue;
        }
        if newlines >= 2 {
            out.push_str("\n\n");
        } else if last_was_space && !out.is_empty() {
            out.push(' ');
        }
        newlines = 0;
        last_was_space = false;
        out.push(c);
    }
    out.trim().to_string()
}

/// Truncate to `max` characters on a char boundary, appending an
/// ellipsis marker when cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}\n\n[… truncated at {max} chars]")
}

/// Pull `{title, url, description}` triples out of a Tavily search
/// response (`results[]` with `content` as the snippet, Phase MM).
fn parse_tavily_results(payload: &Value, limit: usize) -> Vec<Value> {
    payload
        .get("results")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .take(limit)
                .map(|r| {
                    json!({
                        "title": r.get("title").and_then(|t| t.as_str()).unwrap_or(""),
                        "url": r.get("url").and_then(|u| u.as_str()).unwrap_or(""),
                        "description": r.get("content").and_then(|c| c.as_str()).unwrap_or(""),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Pull `{title, url, description}` triples out of a Brave web-search
/// response (`web.results[]`).
fn parse_brave_results(payload: &Value, limit: usize) -> Vec<Value> {
    payload
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .take(limit)
                .map(|r| {
                    json!({
                        "title": r.get("title").and_then(|t| t.as_str()).unwrap_or(""),
                        "url": r.get("url").and_then(|u| u.as_str()).unwrap_or(""),
                        "description": r.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text_strips_tags_and_decodes_entities() {
        let html =
            "<html><body><h1>Title</h1><p>Hello &amp; welcome to &lt;Rust&gt;.</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        // `&amp;` → `&`, `&lt;Rust&gt;` → `<Rust>` (decoding legitimately
        // re-introduces angle brackets as literal text).
        assert!(text.contains("Hello & welcome to <Rust>."));
        // No actual HTML tags survive.
        assert!(!text.contains("<h1>"));
        assert!(!text.contains("<body>"));
        assert!(!text.contains("</p>"));
    }

    #[test]
    fn html_to_text_drops_script_and_style() {
        let html = "<style>.a{color:red}</style><p>visible</p><script>alert(1)</script>";
        let text = html_to_text(html);
        assert!(text.contains("visible"));
        assert!(!text.contains("color:red"));
        assert!(!text.contains("alert(1)"));
    }

    #[tokio::test]
    async fn fetch_bytes_rejects_non_http_scheme() {
        // No network: a non-http(s) scheme must fail fast, before any GET.
        let err = fetch_bytes("ftp://example.com/x").await.unwrap_err();
        assert!(format!("{err}").contains("http(s)"));
    }

    #[test]
    fn looks_like_html_detects_doctype_and_html() {
        assert!(looks_like_html("<!DOCTYPE html><html>"));
        assert!(looks_like_html("  <html>"));
        assert!(!looks_like_html("{\"json\": true}"));
        assert!(!looks_like_html("plain text"));
    }

    #[test]
    fn collapse_whitespace_squeezes_runs() {
        assert_eq!(collapse_whitespace("a    b\t\tc"), "a b c");
        assert_eq!(collapse_whitespace("  trim  me  "), "trim me");
    }

    #[test]
    fn collapse_whitespace_keeps_paragraph_breaks() {
        let out = collapse_whitespace("para one\n\n\n\npara two");
        assert_eq!(out, "para one\n\npara two");
    }

    #[test]
    fn truncate_chars_marks_when_cut() {
        let s = "x".repeat(100);
        let t = truncate_chars(&s, 10);
        assert!(t.starts_with(&"x".repeat(10)));
        assert!(t.contains("truncated"));
        assert_eq!(truncate_chars("short", 10), "short");
    }

    #[test]
    fn parse_brave_results_extracts_triples() {
        let payload = json!({
            "web": { "results": [
                { "title": "Rust", "url": "https://rust-lang.org", "description": "systems lang" },
                { "title": "No URL" }
            ] }
        });
        let results = parse_brave_results(&payload, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["title"], "Rust");
        assert_eq!(results[0]["url"], "https://rust-lang.org");
        assert_eq!(results[1]["url"], ""); // missing fields default to empty
    }

    #[test]
    fn parse_tavily_results_maps_content_to_description() {
        let payload = json!({
            "results": [
                { "title": "Rust", "url": "https://rust-lang.org", "content": "systems lang" }
            ]
        });
        let results = parse_tavily_results(&payload, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], "Rust");
        assert_eq!(results[0]["description"], "systems lang");
    }

    #[test]
    fn isolate_main_prefers_article_subtree() {
        let body = format!(
            "<html><body><nav>menu menu menu</nav><article>{}</article><footer>foot</footer></body></html>",
            "the real content goes here ".repeat(10)
        );
        let main = isolate_main(&body);
        assert!(main.contains("the real content"));
        assert!(!main.contains("menu menu"));
    }

    #[test]
    fn html_to_text_drops_nav_and_footer_chrome() {
        let body = format!(
            "<html><body><nav>NAVLINK</nav><article>{}</article><footer>FOOTSTUFF</footer></body></html>",
            "real article body text ".repeat(12)
        );
        let text = html_to_text(&body);
        assert!(text.contains("real article body text"));
        assert!(!text.contains("NAVLINK"));
        assert!(!text.contains("FOOTSTUFF"));
    }

    #[test]
    fn parse_brave_results_respects_limit_and_missing_key() {
        let payload =
            json!({ "web": { "results": [ {"title":"a"}, {"title":"b"}, {"title":"c"} ] } });
        assert_eq!(parse_brave_results(&payload, 2).len(), 2);
        assert!(parse_brave_results(&json!({}), 5).is_empty());
    }

    #[test]
    fn web_tools_are_safe_class() {
        assert_eq!(WebFetch.classify(), SafetyClass::Safe);
        assert_eq!(WebSearch.classify(), SafetyClass::Safe);
        assert_eq!(WebFetch.name(), "web_fetch");
        assert_eq!(WebSearch.name(), "web_search");
    }
}
