//! Built-in tools for the chat agent: web search (DuckDuckGo) and HTTP(S) page fetch.
//! Used with provider tool-calling (OpenAI Chat Completions, Ollama `/api/chat`, Anthropic Messages). URLs are restricted to reduce SSRF.

use std::net::IpAddr;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use url::Url;

use crate::provider::{ProviderError, ToolDefinition};

const FETCH_MAX_BYTES: usize = 900_000;
const FETCH_TIMEOUT_SECS: u64 = 25;
const SEARCH_QUERY_MAX: usize = 400;
/// DuckDuckGo HTML results page (SERP) — used when the instant-answer JSON API has no snippets.
const DDG_HTML_SERP_MAX_BYTES: usize = 512_000;
const DDG_BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

/// OpenAI function tools offered when agent web tools are enabled.
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "web_search".into(),
            description: Some(
                "Search the public web via DuckDuckGo: uses the instant-answer API when it has a snippet, otherwise HTML search result titles and links (not a full page dump). Good for pointers and headlines; for full articles use fetch_url.".into(),
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "fetch_url".into(),
            description: Some(
                "Fetch a public http(s) URL and return plain-text-ish body (HTML tags stripped, size-capped). Do not use for secrets or authenticated pages.".into(),
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute http or https URL" }
                },
                "required": ["url"]
            }),
        },
    ]
}

fn tool_err(msg: impl Into<String>) -> ProviderError {
    ProviderError::Api(msg.into())
}

fn blocked_host(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_lowercase();
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h == "0.0.0.0"
        || h == "127.0.0.1"
        || h == "::1"
        || h == "metadata.google.internal"
        || h == "169.254.169.254"
    {
        return true;
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_broadcast(),
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local(),
        };
    }
    false
}

/// Only `http:` / `https:` to public-looking hosts (best-effort SSRF guard).
pub fn validate_fetch_url(raw: &str) -> Result<Url, ProviderError> {
    let raw = raw.trim();
    if raw.len() > 2048 {
        return Err(tool_err("URL too long"));
    }
    let u = Url::parse(raw).map_err(|e| tool_err(format!("invalid URL: {e}")))?;
    match u.scheme() {
        "http" | "https" => {}
        s => return Err(tool_err(format!("unsupported URL scheme: {s}"))),
    }
    if u.username() != "" || u.password().is_some() {
        return Err(tool_err("URLs with embedded credentials are not allowed"));
    }
    let host = u
        .host_str()
        .ok_or_else(|| tool_err("URL must include a host"))?;
    if blocked_host(host) {
        return Err(tool_err("URL host is not allowed (private or local addresses blocked)"));
    }
    Ok(u)
}

fn strip_html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len().min(200_000));
    let mut in_tag = false;
    let mut in_quote: Option<char> = None;
    for c in html.chars() {
        if in_tag {
            match c {
                '"' | '\'' => {
                    if in_quote == Some(c) {
                        in_quote = None;
                    } else if in_quote.is_none() {
                        in_quote = Some(c);
                    }
                }
                '>' if in_quote.is_none() => in_tag = false,
                _ => {}
            }
            continue;
        }
        match c {
            '<' => in_tag = true,
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn json_scalar_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn html_unescape_minimal(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Resolve DuckDuckGo redirect URLs (`/l/?uddg=`) to the target URL string.
fn decode_ddg_redirect_url(href: &str) -> String {
    let href = href.trim();
    let parse_query = |u: &Url| -> Option<String> {
        u.query_pairs()
            .find(|(k, _)| k == "uddg")
            .map(|(_, v)| v.into_owned())
    };
    if let Ok(u) = Url::parse(href) {
        if let Some(t) = parse_query(&u) {
            return t;
        }
        return href.to_string();
    }
    if href.starts_with("//") {
        if let Ok(u) = Url::parse(&format!("https:{href}")) {
            if let Some(t) = parse_query(&u) {
                return t;
            }
        }
    }
    href.to_string()
}

/// Parse organic links from DuckDuckGo HTML SERP (`result__a` anchors).
fn parse_ddg_html_serp_links(html: &str, limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pos = 0usize;
    let markers = ["class=\"result__a\"", "class='result__a'"];
    while lines.len() < limit && pos < html.len() {
        let mut next_hit: Option<(usize, usize)> = None;
        for m in markers {
            if let Some(rel) = html[pos..].find(m) {
                let abs = pos + rel;
                next_hit = Some(match next_hit {
                    None => (abs, m.len()),
                    Some((best, _bl)) if abs < best => (abs, m.len()),
                    Some(best) => best,
                });
            }
        }
        let Some((idx, mlen)) = next_hit else {
            break;
        };
        let tail = &html[idx + mlen..];
        let href_needle = "href=";
        let Some(h0) = tail.find(href_needle) else {
            pos = idx + mlen + 4;
            continue;
        };
        let after_eq = &tail[h0 + href_needle.len()..];
        let (delim, rest) = if let Some(r) = after_eq.strip_prefix('"') {
            ('"', r)
        } else if let Some(r) = after_eq.strip_prefix('\'') {
            ('\'', r)
        } else {
            pos = idx + mlen + h0 + 4;
            continue;
        };
        let url_end = rest.find(delim).unwrap_or(0);
        let href_raw = rest.get(..url_end).unwrap_or("");
        let after_url = rest.get(url_end + 1..).unwrap_or("");
        let Some(gt) = after_url.find('>') else {
            pos = idx + mlen + h0 + 20;
            continue;
        };
        let inner = after_url.get(gt + 1..).unwrap_or("");
        let title_end = inner.find("</a>").unwrap_or(inner.len().min(500));
        let title_raw = inner.get(..title_end).unwrap_or("").trim();
        let title = strip_html_to_text(&html_unescape_minimal(title_raw));
        let url = decode_ddg_redirect_url(href_raw);
        if title.len() > 1 && !url.is_empty() {
            lines.push(format!("{}. {title} — {url}", lines.len() + 1));
        }
        let consumed = inner
            .find("</a>")
            .map(|j| j.saturating_add(4))
            .unwrap_or(80)
            .min(4000);
        pos = idx + mlen + h0 + url_end + consumed;
    }
    lines
}

async fn ddg_html_serp_fallback(http: &reqwest::Client, query: &str) -> Result<Vec<String>, ProviderError> {
    let res = http
        .post("https://html.duckduckgo.com/html/")
        .header("User-Agent", DDG_BROWSER_UA)
        .header("Accept", "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .form(&[("q", query), ("b", "")])
        .timeout(Duration::from_secs(25))
        .send()
        .await?
        .error_for_status()?;

    let mut buf: Vec<u8> = Vec::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ProviderError::Http)?;
        if buf.len() + chunk.len() > DDG_HTML_SERP_MAX_BYTES {
            break;
        }
        buf.extend_from_slice(&chunk);
    }

    let html = String::from_utf8_lossy(&buf);
    let lines = parse_ddg_html_serp_links(&html, 12);
    if lines.is_empty() {
        eprintln!(
            "nova: ddg HTML SERP fallback found no result__a links (response_bytes={})",
            buf.len()
        );
    }
    Ok(lines)
}

pub async fn ddg_web_search(http: &reqwest::Client, query: &str) -> Result<String, ProviderError> {
    let q = query.trim();
    if q.is_empty() {
        return Err(tool_err("empty search query"));
    }
    let q: String = q.chars().take(SEARCH_QUERY_MAX).collect();
    let res = http
        .get("https://api.duckduckgo.com/")
        .query(&[
            ("q", q.as_str()),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
        ])
        .header("User-Agent", DDG_BROWSER_UA)
        .timeout(Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?;
    let v: Value = res.json().await?;
    if let Some(err) = v["error"].as_str() {
        return Err(tool_err(format!("search API: {err}")));
    }

    let mut parts: Vec<String> = Vec::new();

    if let Some(a) = v["AbstractText"].as_str().filter(|s| !s.trim().is_empty()) {
        parts.push(format!("Summary: {}", a.trim()));
        if let (Some(src), Some(u)) = (
            v["AbstractSource"].as_str().filter(|s| !s.is_empty()),
            v["AbstractURL"].as_str().filter(|s| !s.is_empty()),
        ) {
            parts.push(format!("Source: {src} — {u}"));
        }
    }

    if let Some(d) = v["Definition"].as_str().filter(|s| !s.trim().is_empty()) {
        let src = v["DefinitionSource"].as_str().unwrap_or("");
        if src.is_empty() {
            parts.push(format!("Definition: {}", d.trim()));
        } else {
            parts.push(format!("Definition ({src}): {}", d.trim()));
        }
    }

    if let Some(a) = json_scalar_to_string(&v["Answer"]) {
        parts.push(format!("Answer: {a}"));
    }

    if let Some(arr) = v["Results"].as_array() {
        let mut hits: Vec<String> = Vec::new();
        for r in arr.iter().take(10) {
            let title = r["Text"]
                .as_str()
                .or_else(|| r["Title"].as_str())
                .unwrap_or("")
                .trim();
            let url = r["FirstURL"]
                .as_str()
                .or_else(|| r["URL"].as_str())
                .unwrap_or("")
                .trim();
            if title.is_empty() {
                continue;
            }
            let line = if url.is_empty() {
                title.to_string()
            } else {
                format!("- {title} ({url})")
            };
            hits.push(line);
        }
        if !hits.is_empty() {
            parts.push(format!("Results:\n{}", hits.join("\n")));
        }
    }

    fn collect_topics(topics: &Value, out: &mut Vec<String>, budget: &mut usize) {
        let Some(arr) = topics.as_array() else {
            return;
        };
        for t in arr {
            if *budget == 0 {
                return;
            }
            if let Some(name) = t["Name"].as_str() {
                if let Some(sub) = t.get("Topics") {
                    out.push(format!("— {name} —"));
                    collect_topics(sub, out, budget);
                    continue;
                }
            }
            if let Some(text) = t["Text"].as_str() {
                let url = t["FirstURL"].as_str().unwrap_or("");
                let line = if url.is_empty() {
                    text.to_string()
                } else {
                    format!("- {text} ({url})")
                };
                out.push(line);
                *budget -= 1;
            }
            if let Some(nested) = t.get("Topics") {
                collect_topics(nested, out, budget);
            }
        }
    }
    let mut budget = 10usize;
    let mut related: Vec<String> = Vec::new();
    collect_topics(&v["RelatedTopics"], &mut related, &mut budget);
    if !related.is_empty() {
        parts.push(format!("Related:\n{}", related.join("\n")));
    }

    if parts.is_empty() {
        let key_hint = v
            .as_object()
            .map(|m| m.keys().take(18).map(|k| k.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        eprintln!(
            "nova: ddg instant-answer JSON had no usable snippets (sample keys: {key_hint:?}); trying HTML SERP fallback"
        );
        match ddg_html_serp_fallback(http, q.as_str()).await {
            Ok(serp) if !serp.is_empty() => {
                parts.push(format!(
                    "Web results (DuckDuckGo; instant API had no abstract for this query):\n{}",
                    serp.join("\n")
                ));
            }
            Ok(_) => {}
            Err(e) => eprintln!("nova: ddg HTML SERP fallback failed: {e}"),
        }
    }

    if parts.is_empty() {
        Ok(
            "No usable search snippets from DuckDuckGo for this query. The instant-answer API rarely covers broad or time-sensitive news; try a more specific factual question, or use fetch_url on a known article URL."
                .into(),
        )
    } else {
        Ok(parts.join("\n\n"))
    }
}

pub async fn fetch_url_text(http: &reqwest::Client, raw_url: &str) -> Result<String, ProviderError> {
    let url = validate_fetch_url(raw_url)?;
    let res = http
        .get(url.clone())
        .header(
            "User-Agent",
            "Mozilla/5.0 (compatible; NovaCompanion/1.0; +https://github.com/)",
        )
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .send()
        .await?
        .error_for_status()?;

    let mut buf: Vec<u8> = Vec::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ProviderError::Http)?;
        if buf.len() + chunk.len() > FETCH_MAX_BYTES {
            break;
        }
        buf.extend_from_slice(&chunk);
    }

    let text = String::from_utf8_lossy(&buf);
    let body = if text.trim_start().to_lowercase().starts_with("<!doctype html")
        || text.contains("<html")
        || text.contains("<HTML")
    {
        strip_html_to_text(&text)
    } else {
        text.into_owned()
    };

    let max_chars = 48_000usize;
    let out: String = if body.chars().count() > max_chars {
        body.chars().take(max_chars).collect::<String>() + "… [truncated]"
    } else {
        body
    };

    Ok(format!("URL: {}\n\n{}", url, out))
}

/// Run one built-in tool by name; returns text for the `tool` message.
pub async fn run_builtin_tool(http: &reqwest::Client, name: &str, arguments_json: &str) -> Result<String, ProviderError> {
    let v: Value = serde_json::from_str(arguments_json).map_err(|e| tool_err(format!("bad tool JSON: {e}")))?;
    match name.trim() {
        "web_search" => {
            let q = v["query"].as_str().unwrap_or("").trim();
            ddg_web_search(http, q).await
        }
        "fetch_url" => {
            let u = v["url"].as_str().unwrap_or("").trim();
            fetch_url_text(http, u).await
        }
        other => Err(tool_err(format!("unknown tool: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_ddg_redirect_extracts_uddg() {
        let u = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fnews.example.com%2Fstory";
        let out = decode_ddg_redirect_url(u);
        assert_eq!(out, "https://news.example.com/story");
    }

    #[test]
    fn parse_ddg_html_serp_extracts_title_and_decoded_url() {
        let html = r#"<div class="web-result"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage">Example headline</a></div>"#;
        let lines = parse_ddg_html_serp_links(html, 5);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Example headline"), "{lines:?}");
        assert!(lines[0].contains("https://example.com/page"), "{lines:?}");
    }
}
