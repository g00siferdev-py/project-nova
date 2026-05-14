//! Built-in tools for the chat agent: web search (DuckDuckGo), HTTP(S) fetch, HTTPS `http_request`
//! (custom headers / methods for authenticated APIs), and optional sandboxed read/write under the
//! app workspace directory.
//! Used with provider tool-calling (OpenAI Chat Completions, Ollama `/api/chat`, Anthropic Messages).
//! URLs are restricted to reduce SSRF; workspace paths are jailed to `{data_dir}/workspace`.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{HeaderName, HeaderValue, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use url::Url;

use crate::provider::{ProviderError, ToolDefinition};

const FETCH_MAX_BYTES: usize = 900_000;
const FETCH_TIMEOUT_SECS: u64 = 25;
const HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
const HTTP_REQUEST_MAX_REQ_BODY_BYTES: usize = 512_000;
const HTTP_REQUEST_MAX_RESPONSE_BYTES: usize = 900_000;
const HTTP_REQUEST_RESPONSE_BODY_MAX_CHARS: usize = 96_000;
const HTTP_REQUEST_MAX_HEADER_COUNT: usize = 40;
const HTTP_REQUEST_MAX_HEADER_NAME_LEN: usize = 256;
const HTTP_REQUEST_MAX_HEADER_VALUE_LEN: usize = 16_384;
const NOVA_HTTP_REQUEST_UA: &str = concat!("Nova/", env!("CARGO_PKG_VERSION"), " (+https://github.com/)");
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
        ToolDefinition {
            name: "http_request".into(),
            description: Some(
                "Perform an HTTPS request with optional method, custom headers, and body. Returns JSON: { status, statusText, headers, body }. Use for authenticated APIs (e.g. Authorization: Bearer <token>). Only https:// URLs; private/local hosts are blocked (SSRF guard). CR/LF not allowed in header names or values.".into(),
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "HTTPS URL (https:// only)" },
                    "method": {
                        "type": "string",
                        "description": "HTTP method",
                        "enum": ["GET", "POST", "PUT", "DELETE", "PATCH"]
                    },
                    "headers": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Optional header map (string values only), e.g. Authorization Bearer"
                    },
                    "body": { "type": "string", "description": "Optional request body (e.g. JSON as a string)" }
                },
                "required": ["url"]
            }),
        },
    ]
}

const WORKSPACE_READ_MAX_BYTES: usize = 900_000;
const WORKSPACE_WRITE_MAX_BYTES: usize = 900_000;
const WORKSPACE_LIST_MAX_ENTRIES: usize = 250;
const WORKSPACE_REL_PATH_MAX: usize = 2048;

/// Tools offered when agent workspace access is enabled (paths are relative to the workspace root).
pub fn workspace_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "workspace_read_file".into(),
            description: Some(
                "Read a UTF-8 text file inside the Nova workspace. Path is relative to the workspace root (forward slashes, no ..).".into(),
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path under the workspace" },
                    "max_bytes": { "type": "integer", "description": "Optional read cap (bytes); defaults to a safe maximum" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "workspace_write_file".into(),
            description: Some(
                "Create or overwrite a UTF-8 text file in the workspace. Parent directories are created as needed. Path is relative (no ..).".into(),
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path under the workspace" },
                    "content": { "type": "string", "description": "Full file contents (UTF-8 text)" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "workspace_list_directory".into(),
            description: Some(
                "List immediate children of a directory inside the workspace (non-recursive). Path is relative; use \".\" for the workspace root.".into(),
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative directory path (default \".\")" }
                },
                "required": []
            }),
        },
    ]
}

/// Build `workspace_root/rel` with `rel` sanitized (no `..`, no absolute paths, forward slashes only).
pub fn resolve_workspace_subpath(workspace_root: &Path, rel: &str) -> Result<PathBuf, ProviderError> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err(tool_err("path is empty"));
    }
    if rel.len() > WORKSPACE_REL_PATH_MAX {
        return Err(tool_err("path too long"));
    }
    if rel.starts_with('/') {
        return Err(tool_err("path must be relative (no leading /)"));
    }
    if rel.contains('\\') {
        return Err(tool_err("path must use forward slashes only"));
    }
    let mut out = workspace_root.to_path_buf();
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(tool_err("path must not contain '..' segments"));
        }
        out.push(seg);
    }
    Ok(out)
}

fn workspace_root_canonical(workspace_root: &Path) -> Result<PathBuf, ProviderError> {
    std::fs::canonicalize(workspace_root).map_err(|e| tool_err(format!("workspace root: {e}")))
}

/// Verifies `path` (built under the workspace) does not escape via symlinks.
fn assert_path_in_workspace(workspace_root: &Path, path: &Path) -> Result<(), ProviderError> {
    let root_canon = workspace_root_canonical(workspace_root)?;
    if path.exists() {
        let p = std::fs::canonicalize(path).map_err(|e| tool_err(format!("path: {e}")))?;
        if !p.starts_with(&root_canon) {
            return Err(tool_err("path resolves outside the workspace"));
        }
        return Ok(());
    }
    let mut anc = path.to_path_buf();
    loop {
        if anc.as_os_str().is_empty() {
            return Err(tool_err("invalid path"));
        }
        if anc.exists() {
            let a = std::fs::canonicalize(&anc).map_err(|e| tool_err(format!("path: {e}")))?;
            if !a.starts_with(&root_canon) {
                return Err(tool_err("path resolves outside the workspace"));
            }
            return Ok(());
        }
        if anc == workspace_root {
            return Ok(());
        }
        if !anc.pop() {
            return Err(tool_err("path escapes the workspace"));
        }
    }
}

fn workspace_read_file(workspace_root: &Path, rel: &str, max_bytes: Option<u64>) -> Result<String, ProviderError> {
    let path = resolve_workspace_subpath(workspace_root, rel)?;
    assert_path_in_workspace(workspace_root, &path)?;
    let cap = max_bytes
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(WORKSPACE_READ_MAX_BYTES)
        .min(WORKSPACE_READ_MAX_BYTES);
    let meta = std::fs::metadata(&path).map_err(|e| tool_err(format!("read_file: {e}")))?;
    if !meta.is_file() {
        return Err(tool_err("path is not a regular file"));
    }
    let len = meta.len() as usize;
    if len > cap {
        return Err(tool_err(format!(
            "file is {} bytes (max {})",
            meta.len(),
            cap
        )));
    }
    let bytes = std::fs::read(&path).map_err(|e| tool_err(format!("read_file: {e}")))?;
    let text = String::from_utf8(bytes).map_err(|_| tool_err("file is not valid UTF-8"))?;
    Ok(text)
}

fn workspace_write_file(workspace_root: &Path, rel: &str, content: &str) -> Result<String, ProviderError> {
    let path = resolve_workspace_subpath(workspace_root, rel)?;
    if content.as_bytes().len() > WORKSPACE_WRITE_MAX_BYTES {
        return Err(tool_err(format!(
            "content exceeds {} bytes",
            WORKSPACE_WRITE_MAX_BYTES
        )));
    }
    if let Some(parent) = path.parent() {
        assert_path_in_workspace(workspace_root, parent)?;
        std::fs::create_dir_all(parent).map_err(|e| tool_err(format!("create_dir_all: {e}")))?;
    }
    assert_path_in_workspace(workspace_root, &path)?;
    std::fs::write(&path, content.as_bytes()).map_err(|e| tool_err(format!("write_file: {e}")))?;
    Ok(format!("Wrote {} bytes to {}", content.as_bytes().len(), rel.trim()))
}

fn workspace_list_directory(workspace_root: &Path, rel: &str) -> Result<String, ProviderError> {
    let rel = if rel.trim().is_empty() { "." } else { rel };
    let path = resolve_workspace_subpath(workspace_root, rel)?;
    assert_path_in_workspace(workspace_root, &path)?;
    let meta = std::fs::metadata(&path).map_err(|e| tool_err(format!("list_directory: {e}")))?;
    if !meta.is_dir() {
        return Err(tool_err("path is not a directory"));
    }
    let read = std::fs::read_dir(&path).map_err(|e| tool_err(format!("list_directory: {e}")))?;
    let mut lines: Vec<String> = Vec::new();
    for (i, ent) in read.enumerate() {
        if i >= WORKSPACE_LIST_MAX_ENTRIES {
            lines.push(format!("… (listing truncated after {WORKSPACE_LIST_MAX_ENTRIES} entries)"));
            break;
        }
        let ent = ent.map_err(|e| tool_err(format!("list_directory: {e}")))?;
        let ty = ent
            .file_type()
            .map(|t| {
                if t.is_dir() {
                    "dir"
                } else if t.is_symlink() {
                    "symlink"
                } else {
                    "file"
                }
            })
            .unwrap_or("?");
        let name = ent.file_name().to_string_lossy().into_owned();
        lines.push(format!("{ty}\t{name}"));
    }
    if lines.is_empty() {
        Ok("(empty directory)".into())
    } else {
        Ok(lines.join("\n"))
    }
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

/// `https:` only, same host rules as [`validate_fetch_url`] (SSRF guard).
fn validate_agent_https_url(raw: &str) -> Result<Url, ProviderError> {
    let raw = raw.trim();
    if raw.len() > 2048 {
        return Err(tool_err("URL too long (max 2048 characters)"));
    }
    let u = Url::parse(raw).map_err(|e| tool_err(format!("invalid URL: {e}")))?;
    match u.scheme() {
        "https" => {}
        "http" => {
            return Err(tool_err(
                "http_request only allows https:// URLs (plain http is blocked)",
            ));
        }
        s => return Err(tool_err(format!("unsupported URL scheme: {s} (only https)"))),
    }
    if u.username() != "" || u.password().is_some() {
        return Err(tool_err("URLs with embedded credentials are not allowed; put tokens in headers instead"));
    }
    let host = u
        .host_str()
        .ok_or_else(|| tool_err("URL must include a host"))?;
    if blocked_host(host) {
        return Err(tool_err("URL host is not allowed (private or local addresses blocked)"));
    }
    Ok(u)
}

/// Parse `headers` object into validated HTTP headers (rejects header injection / invalid tokens).
fn parse_http_request_headers(headers_val: Option<&Value>) -> Result<Vec<(HeaderName, HeaderValue)>, ProviderError> {
    let Some(v) = headers_val else {
        return Ok(Vec::new());
    };
    let Some(obj) = v.as_object() else {
        return Err(tool_err("headers must be a JSON object of string keys to string values"));
    };
    if obj.len() > HTTP_REQUEST_MAX_HEADER_COUNT {
        return Err(tool_err(format!(
            "too many headers (max {HTTP_REQUEST_MAX_HEADER_COUNT})"
        )));
    }
    let mut out = Vec::with_capacity(obj.len());
    for (k, val) in obj {
        let name = k.trim();
        if name.is_empty() {
            return Err(tool_err("header names must not be empty"));
        }
        if name.len() > HTTP_REQUEST_MAX_HEADER_NAME_LEN {
            return Err(tool_err(format!(
                "header name too long (max {HTTP_REQUEST_MAX_HEADER_NAME_LEN} characters)"
            )));
        }
        if name.contains('\n') || name.contains('\r') || name.contains('\0') {
            return Err(tool_err(format!(
                "header name `{name}` contains invalid characters (CR, LF, and NUL are not allowed)"
            )));
        }
        let value_str = match val {
            Value::String(s) => s.as_str(),
            _ => {
                return Err(tool_err(format!(
                    "header `{name}` value must be a string (JSON string)"
                )));
            }
        };
        if value_str.len() > HTTP_REQUEST_MAX_HEADER_VALUE_LEN {
            return Err(tool_err(format!(
                "header `{name}` value too long (max {HTTP_REQUEST_MAX_HEADER_VALUE_LEN} bytes)"
            )));
        }
        if value_str.contains('\n') || value_str.contains('\r') || value_str.contains('\0') {
            return Err(tool_err(format!(
                "header `{name}` contains invalid characters (CR, LF, and NUL are not allowed — possible header injection)"
            )));
        }
        let hn = HeaderName::from_str(name).map_err(|_| tool_err(format!("invalid HTTP header name: `{name}`")))?;
        let hv =
            HeaderValue::from_str(value_str).map_err(|_| tool_err(format!("invalid HTTP header value for `{name}`")))?;
        out.push((hn, hv));
    }
    Ok(out)
}

fn http_method_from_json(v: &Value) -> Result<Method, ProviderError> {
    let s = v
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("GET")
        .trim()
        .to_ascii_uppercase();
    match s.as_str() {
        "" | "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "DELETE" => Ok(Method::DELETE),
        "PATCH" => Ok(Method::PATCH),
        other => Err(tool_err(format!(
            "unsupported HTTP method `{other}` (allowed: GET, POST, PUT, DELETE, PATCH)"
        ))),
    }
}

fn response_headers_to_json_map(headers: &reqwest::header::HeaderMap) -> serde_json::Map<String, Value> {
    let mut acc: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in headers.iter() {
        let key = k.as_str().to_string();
        let val = v
            .to_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|_| String::from_utf8_lossy(v.as_bytes()).into_owned());
        acc.entry(key).or_default().push(val);
    }
    let mut out = serde_json::Map::new();
    for (k, vals) in acc {
        out.insert(k, json!(vals.join(", ")));
    }
    out
}

async fn http_request_tool(http: &reqwest::Client, v: &Value) -> Result<String, ProviderError> {
    let raw_url = v
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| tool_err("http_request: `url` is required and must be a string"))?
        .trim();
    if raw_url.is_empty() {
        return Err(tool_err("http_request: url is empty"));
    }
    let url = validate_agent_https_url(raw_url)?;
    let method = http_method_from_json(v)?;
    let header_pairs = parse_http_request_headers(v.get("headers"))?;
    let user_agent_set = header_pairs
        .iter()
        .any(|(n, _)| n.as_str().eq_ignore_ascii_case("user-agent"));
    let body_opt: Option<&str> = match v.get("body") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => {
            return Err(tool_err(
                "http_request: `body` must be a JSON string or null/omitted (for JSON APIs, pass a stringified object as the body string)",
            ));
        }
    };
    let body_bytes: Option<Vec<u8>> = if let Some(b) = body_opt {
        if b.as_bytes().len() > HTTP_REQUEST_MAX_REQ_BODY_BYTES {
            return Err(tool_err(format!(
                "http_request: body exceeds {} bytes",
                HTTP_REQUEST_MAX_REQ_BODY_BYTES
            )));
        }
        Some(b.as_bytes().to_vec())
    } else {
        None
    };

    let mut rb = http
        .request(method, url.as_str())
        .timeout(Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS));
    if !user_agent_set {
        rb = rb.header(USER_AGENT, NOVA_HTTP_REQUEST_UA);
    }
    for (name, value) in header_pairs {
        rb = rb.header(name, value);
    }
    if let Some(bytes) = body_bytes {
        rb = rb.body(bytes);
    }

    let res = match rb.send().await {
        Ok(r) => r,
        Err(e) => {
            if e.is_timeout() {
                return Err(tool_err(
                    "http_request timed out — try again later, increase server responsiveness, or narrow the request",
                ));
            }
            if e.is_connect() {
                return Err(tool_err(format!(
                    "http_request connection failed — check the URL, DNS, firewall, and TLS: {e}"
                )));
            }
            if e.is_request() {
                return Err(tool_err(format!("http_request could not be built or sent: {e}")));
            }
            return Err(ProviderError::Http(e));
        }
    };

    let status = res.status();
    let mut status_text = status
        .canonical_reason()
        .unwrap_or("Non-standard status")
        .to_string();
    if status == StatusCode::TOO_MANY_REQUESTS {
        status_text.push_str(" — Rate limited by the server; slow down and honor Retry-After if present in headers.");
    } else if status == StatusCode::SERVICE_UNAVAILABLE
        || status == StatusCode::BAD_GATEWAY
        || status == StatusCode::GATEWAY_TIMEOUT
    {
        status_text.push_str(" — Upstream overload or outage; retry after a delay.");
    }

    let resp_headers = response_headers_to_json_map(res.headers());

    let mut buf: Vec<u8> = Vec::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            if e.is_timeout() {
                tool_err("http_request timed out while reading the response body")
            } else {
                ProviderError::Http(e)
            }
        })?;
        if buf.len() + chunk.len() > HTTP_REQUEST_MAX_RESPONSE_BYTES {
            break;
        }
        buf.extend_from_slice(&chunk);
    }

    let mut body_out = String::from_utf8_lossy(&buf).into_owned();
    if body_out.chars().count() > HTTP_REQUEST_RESPONSE_BODY_MAX_CHARS {
        body_out = body_out
            .chars()
            .take(HTTP_REQUEST_RESPONSE_BODY_MAX_CHARS)
            .collect::<String>()
            + "\n… [truncated by Nova http_request output limit]";
    }

    let payload = json!({
        "status": status.as_u16(),
        "statusText": status_text,
        "headers": resp_headers,
        "body": body_out,
    });
    serde_json::to_string(&payload).map_err(|e| tool_err(format!("http_request: failed to serialize response: {e}")))
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
pub async fn run_builtin_tool(
    http: &reqwest::Client,
    workspace_root: Option<&Path>,
    name: &str,
    arguments_json: &str,
) -> Result<String, ProviderError> {
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
        "http_request" => http_request_tool(http, &v).await,
        "workspace_read_file" => {
            let root = workspace_root.ok_or_else(|| tool_err("workspace tools are not enabled"))?;
            let p = v["path"].as_str().unwrap_or("").trim();
            let max_bytes = v.get("max_bytes").and_then(|x| x.as_u64());
            let text = workspace_read_file(root, p, max_bytes)?;
            const TOOL_OUT_MAX: usize = 48_000;
            if text.chars().count() > TOOL_OUT_MAX {
                Ok(text.chars().take(TOOL_OUT_MAX).collect::<String>() + "\n… [truncated]")
            } else {
                Ok(text)
            }
        }
        "workspace_write_file" => {
            let root = workspace_root.ok_or_else(|| tool_err("workspace tools are not enabled"))?;
            let p = v["path"].as_str().unwrap_or("").trim();
            let c = v["content"].as_str().unwrap_or("");
            workspace_write_file(root, p, c)
        }
        "workspace_list_directory" => {
            let root = workspace_root.ok_or_else(|| tool_err("workspace tools are not enabled"))?;
            let p = v["path"].as_str().unwrap_or("").trim();
            workspace_list_directory(root, p)
        }
        other => Err(tool_err(format!("unknown tool: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_ddg_redirect_extracts_uddg() {
        let u = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fnews.example.com%2Fstory";
        let out = decode_ddg_redirect_url(u);
        assert_eq!(out, "https://news.example.com/story");
    }

    #[test]
    fn https_url_validator_rejects_http() {
        let err = validate_agent_https_url("http://example.com/path").unwrap_err();
        assert!(err.to_string().to_lowercase().contains("https"), "{err}");
    }

    #[test]
    fn parse_http_request_headers_rejects_crlf_in_value() {
        let v = json!({ "Authorization": "Bearer x\nX-Injected: 1" });
        assert!(parse_http_request_headers(Some(&v)).is_err());
    }

    #[test]
    fn parse_http_request_headers_accepts_bearer_shape() {
        let v = json!({ "Authorization": "Bearer test_token_example" });
        let h = parse_http_request_headers(Some(&v)).unwrap();
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn workspace_rejects_dotdot() {
        let tmp = std::env::temp_dir();
        let err = resolve_workspace_subpath(&tmp, "a/../../passwd").unwrap_err();
        assert!(err.to_string().contains(".."), "{err}");
    }

    #[test]
    fn workspace_resolves_nested_path() {
        let tmp = std::env::temp_dir().join("nova-ws-resolve-test");
        let _ = std::fs::create_dir_all(&tmp);
        let p = resolve_workspace_subpath(&tmp, "./notes/./file.txt").unwrap();
        assert!(p.ends_with("file.txt"), "{p:?}");
        let _ = std::fs::remove_dir_all(&tmp);
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
