//! `database_query` agent tool: SQLite access to `.db` / `.sqlite` files under the agent workspace
//! or (when enabled) the Nova **app data directory** — the same resolved path as the live MemoryAnchor
//! database (`NOVA_DATA_DIR`, portable `data/` next to the executable, or OS app data). Read-only by
//! default; optional writes behind a separate settings toggle.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{params_from_iter, types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Map, Value};

use crate::agent_tools::{
    assert_path_contained_in, assert_path_in_workspace, resolve_workspace_subpath, tool_err,
};
use crate::provider::{ProviderError, ToolDefinition};

/// Synced to SQLite `preferences` for MemoryAnchor / external readers (`nova.database.allow_write`).
pub const PREF_DATABASE_ALLOW_WRITE: &str = "nova.database.allow_write";

/// Synced to SQLite `preferences` (`nova.database.allow_app_data`).
pub const PREF_DATABASE_APP_DATA: &str = "nova.database.allow_app_data";

const MAX_SQL_CHARS: usize = 262_144;
const DEFAULT_MAX_ROWS: u32 = 1000;
const HARD_MAX_ROWS_CAP: u32 = 5000;
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const BUSY_MAX_ATTEMPTS: u32 = 12;
const BUSY_BASE_MS: u64 = 25;

/// Always blocked (destructive or unsafe for the agent sandbox).
const KEYWORDS_ALWAYS_DENY: &[&str] = &[
    "DROP", "ALTER", "CREATE", "ATTACH", "DETACH", "VACUUM", "PRAGMA", "REINDEX",
];

/// Blocked unless `database_allow_write` is true.
const KEYWORDS_WRITE_DATA: &[&str] = &["INSERT", "UPDATE", "DELETE", "REPLACE"];

pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "database_query".into(),
        description: Some(
            "Run a single SQLite statement on a .db/.sqlite file. Set location to \"workspace\" (default) for files under the agent workspace, or \"app_data\" for files in the Nova data directory (same folder as nova_memory.sqlite — respects NOVA_DATA_DIR and portable data/). Read-only by default; INSERT/UPDATE/DELETE/REPLACE need 'Allow database writes'. DROP/ALTER/CREATE/ATTACH/PRAGMA/VACUUM are always blocked. Returns JSON: success, rows, columns, rowCount, error, execution_time_ms.".into(),
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "\"workspace\" (default) or \"app_data\" (Nova data directory; requires separate Settings toggle)"
                },
                "db_path": { "type": "string", "description": "workspace: relative path under workspace. app_data: filename only (e.g. nova_memory.sqlite), no subdirectories" },
                "query": { "type": "string", "description": "Single SQL statement (use .tables / .schema meta shortcuts if needed)" },
                "params": {
                    "type": "array",
                    "items": { "type": ["string", "number", "null"] },
                    "description": "Optional bound parameters for ? placeholders"
                },
                "max_rows": { "type": "integer", "description": "Max rows to return for SELECT-like queries (default 1000, hard cap 5000)" }
            },
            "required": ["db_path", "query"]
        }),
    }]
}

fn output_err(msg: impl Into<String>) -> String {
    json!({
        "success": false,
        "error": msg.into(),
    })
    .to_string()
}

fn output_ok(
    rows: Vec<Map<String, Value>>,
    columns: Vec<String>,
    row_count: usize,
    elapsed_ms: u128,
) -> String {
    json!({
        "success": true,
        "rows": rows,
        "columns": columns,
        "rowCount": row_count,
        "execution_time_ms": elapsed_ms,
    })
    .to_string()
}

/// Strip `'` / `"` string/identifier literals and SQL comments so keyword scans ignore content inside them.
fn strip_sql_comments_and_strings(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let mut in_squote = false;
    let mut in_dquote = false;
    let mut in_line = false;
    let mut in_block = false;
    while i < chars.len() {
        let c = chars[i];
        if in_line {
            if c == '\n' {
                in_line = false;
                out.push(' ');
            }
            i += 1;
            continue;
        }
        if in_block {
            if c == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                in_block = false;
                i += 2;
                out.push(' ');
                continue;
            }
            i += 1;
            continue;
        }
        if in_squote {
            out.push(' ');
            if c == '\'' {
                if i + 1 < chars.len() && chars[i + 1] == '\'' {
                    i += 2;
                    continue;
                }
                in_squote = false;
            }
            i += 1;
            continue;
        }
        if in_dquote {
            out.push(' ');
            if c == '"' {
                in_dquote = false;
            }
            i += 1;
            continue;
        }
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            in_line = true;
            i += 2;
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            in_block = true;
            i += 2;
            continue;
        }
        if c == '\'' {
            in_squote = true;
            out.push(' ');
            i += 1;
            continue;
        }
        if c == '"' {
            in_dquote = true;
            out.push(' ');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn sql_tokens_for_keyword_scan(sql: &str) -> Vec<String> {
    let cleaned = strip_sql_comments_and_strings(sql);
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(cur);
            cur = String::new();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn assert_single_statement(sql: &str) -> Result<(), ProviderError> {
    let s = sql.trim().trim_end_matches(';').trim();
    if s.contains(';') {
        return Err(tool_err(
            "only a single SQL statement is allowed (no multiple statements separated by `;`)",
        ));
    }
    Ok(())
}

fn normalize_meta_command(query: &str) -> Result<String, ProviderError> {
    let t = query.trim();
    if t.eq_ignore_ascii_case(".tables") {
        return Ok(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*' ORDER BY name"
                .into(),
        );
    }
    let prefix = ".schema";
    if t.len() >= prefix.len() && t[..prefix.len()].eq_ignore_ascii_case(prefix) {
        let rest = t[prefix.len()..].trim();
        if rest.is_empty() {
            return Ok(
                "SELECT name, type, sql FROM sqlite_master WHERE sql IS NOT NULL AND type IN ('table','view') AND name NOT GLOB 'sqlite_*' ORDER BY name"
                    .into(),
            );
        }
        if !rest.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(tool_err(
                ".schema <table> only accepts a simple alphanumeric/underscore table name",
            ));
        }
        return Ok(format!(
            "SELECT name, type, sql FROM sqlite_master WHERE type IN ('table','view') AND name = '{rest}'"
        ));
    }
    Ok(query.to_string())
}

fn check_keyword_policy(tokens: &[String], allow_write: bool) -> Result<(), ProviderError> {
    let upper: Vec<String> = tokens.iter().map(|w| w.to_uppercase()).collect();
    for kw in KEYWORDS_ALWAYS_DENY {
        if upper.iter().any(|t| t == kw) {
            return Err(tool_err(format!(
                "blocked unsafe SQL keyword `{kw}` (DROP/ALTER/CREATE/ATTACH/PRAGMA/VACUUM and similar are not allowed)"
            )));
        }
    }
    if !allow_write {
        for kw in KEYWORDS_WRITE_DATA {
            if upper.iter().any(|t| t == kw) {
                return Err(tool_err("Write operations blocked in read-only mode"));
            }
        }
    }
    Ok(())
}

fn uses_write_data_keywords(tokens: &[String]) -> bool {
    let upper: Vec<String> = tokens.iter().map(|w| w.to_uppercase()).collect();
    KEYWORDS_WRITE_DATA
        .iter()
        .any(|&kw| upper.iter().any(|t| t == kw))
}

fn map_sqlite_error(e: rusqlite::Error) -> String {
    let s = e.to_string();
    if let Some(name) = s.strip_prefix("no such table: ") {
        let name = name.trim();
        return format!("Table '{name}' not found");
    }
    if s.contains("no such column") {
        return s;
    }
    let lower = s.to_lowercase();
    if lower.contains("database is locked") || lower.contains("busy") {
        return "Database locked by another process — close other connections or retry shortly".into();
    }
    s
}

fn json_to_sqlite_value(v: &Value) -> Result<rusqlite::types::Value, ProviderError> {
    match v {
        Value::Null => Ok(rusqlite::types::Value::Null),
        Value::Bool(b) => Ok(rusqlite::types::Value::Integer(if *b { 1 } else { 0 })),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(rusqlite::types::Value::Integer(i))
            } else if let Some(u) = n.as_u64() {
                Ok(rusqlite::types::Value::Integer(i64::try_from(u).map_err(|_| {
                    tool_err("numeric parameter does not fit in SQLite integer")
                })?))
            } else if let Some(f) = n.as_f64() {
                Ok(rusqlite::types::Value::Real(f))
            } else {
                Err(tool_err("invalid JSON number in params"))
            }
        }
        Value::String(s) => Ok(rusqlite::types::Value::Text(s.clone())),
        _ => Err(tool_err("params array may only contain string, number, or null")),
    }
}

fn cell_to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => json!(format!("<blob {} bytes>", b.len())),
    }
}

fn validate_db_extension(path: &Path) -> Result<(), ProviderError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "db" && ext != "sqlite" {
        return Err(tool_err(
            "db_path must end with .db or .sqlite (under the workspace)",
        ));
    }
    Ok(())
}

fn open_connection(path: &Path, read_only: bool) -> Result<Connection, rusqlite::Error> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    Connection::open_with_flags(path, flags)
}

/// Single-segment `.db` / `.sqlite` filename under the resolved Nova data directory (no subpaths).
fn resolve_app_data_sqlite_path(data_directory: &Path, name: &str) -> Result<PathBuf, ProviderError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(tool_err("database_query: db_path is empty"));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(tool_err(
            "database_query app_data db_path must be a single file name (no subdirectories), e.g. nova_memory.sqlite",
        ));
    }
    let p = data_directory.join(name);
    validate_db_extension(&p)?;
    Ok(p)
}

/// Synchronous entry (call from `spawn_blocking`). Returns JSON string for the tool message.
pub fn run_database_query_sync(
    workspace_root: Option<&Path>,
    workspace_database_enabled: bool,
    data_directory: &Path,
    app_data_database_enabled: bool,
    database_allow_write: bool,
    args: &Value,
) -> Result<String, ProviderError> {
    let started_total = Instant::now();
    let db_rel = args
        .get("db_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| tool_err("database_query: db_path is required"))?
        .trim();
    if db_rel.is_empty() {
        return Ok(output_err("database_query: db_path is empty"));
    }
    let raw_query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| tool_err("database_query: query is required"))?;
    if raw_query.len() > MAX_SQL_CHARS {
        return Ok(output_err(format!(
            "database_query: query exceeds {MAX_SQL_CHARS} characters"
        )));
    }

    let location = args
        .get("location")
        .and_then(|v| v.as_str())
        .unwrap_or("workspace")
        .trim()
        .to_ascii_lowercase();

    let sql = match normalize_meta_command(raw_query) {
        Ok(s) => s,
        Err(e) => return Ok(output_err(e.to_string())),
    };

    if let Err(e) = assert_single_statement(&sql) {
        return Ok(output_err(e.to_string()));
    }

    let tokens = sql_tokens_for_keyword_scan(&sql);
    if let Err(e) = check_keyword_policy(&tokens, database_allow_write) {
        return Ok(output_err(e.to_string()));
    }

    let max_rows = args
        .get("max_rows")
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(DEFAULT_MAX_ROWS))
        .unwrap_or(DEFAULT_MAX_ROWS)
        .min(HARD_MAX_ROWS_CAP)
        .max(1);

    let params_json: Vec<Value> = match args.get("params") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => {
            return Ok(output_err(
                "database_query: params must be an array of string, number, or null",
            ));
        }
    };
    let mut sqlite_params: Vec<rusqlite::types::Value> = Vec::with_capacity(params_json.len());
    for p in &params_json {
        sqlite_params.push(json_to_sqlite_value(p)?);
    }

    let db_path = match location.as_str() {
        "" | "workspace" => {
            if !workspace_database_enabled {
                return Ok(output_err(
                    "database_query location=workspace is not enabled — turn on workspace tools in Settings",
                ));
            }
            let root = match workspace_root {
                Some(r) => r,
                None => {
                    return Ok(output_err(
                        "database_query location=workspace requires workspace tools to be enabled",
                    ));
                }
            };
            let p = match resolve_workspace_subpath(root, db_rel) {
                Ok(p) => p,
                Err(e) => return Ok(output_err(e.to_string())),
            };
            if let Err(e) = validate_db_extension(&p) {
                return Ok(output_err(e.to_string()));
            }
            if !p.exists() {
                return Ok(output_err(format!(
                    "database file not found at workspace path: {db_rel}"
                )));
            }
            if let Err(e) = assert_path_in_workspace(root, &p) {
                return Ok(output_err(e.to_string()));
            }
            p
        }
        "app_data" | "appdata" => {
            if !app_data_database_enabled {
                return Ok(output_err(
                    "database_query location=app_data is not enabled — turn on \"App data directory databases\" in Settings",
                ));
            }
            let p = match resolve_app_data_sqlite_path(data_directory, db_rel) {
                Ok(p) => p,
                Err(e) => return Ok(output_err(e.to_string())),
            };
            if let Err(e) = assert_path_contained_in(data_directory, &p, "Nova data directory") {
                return Ok(output_err(e.to_string()));
            }
            if !p.exists() {
                return Ok(output_err(format!(
                    "database file not found in Nova data directory: {db_rel}"
                )));
            }
            p
        }
        other => {
            return Ok(output_err(format!(
                "database_query: unknown location `{other}` (use \"workspace\" or \"app_data\")"
            )));
        }
    };

    let write_data = uses_write_data_keywords(&tokens);
    let use_readonly_connection = !write_data;

    if write_data && database_allow_write {
        let preview: String = sql.chars().take(200).collect();
        eprintln!(
            "nova: database_query WRITE ({}) at {:?} — {}",
            location,
            std::time::SystemTime::now(),
            preview.replace('\n', " ")
        );
    }

    let mut last_err: Option<rusqlite::Error> = None;
    for attempt in 0..BUSY_MAX_ATTEMPTS {
        let result = (|| -> Result<String, rusqlite::Error> {
            let conn = open_connection(&db_path, use_readonly_connection)?;
            conn.busy_timeout(Duration::from_secs(8))?;
            let t0 = Instant::now();
            conn.progress_handler(1, Some(move || t0.elapsed() >= QUERY_TIMEOUT));

            let out = if write_data {
                let changes = conn.execute(&sql, params_from_iter(sqlite_params.clone()))?;
                output_ok(
                    Vec::<Map<String, Value>>::new(),
                    Vec::new(),
                    changes,
                    started_total.elapsed().as_millis(),
                )
            } else {
                let mut stmt = conn.prepare(&sql)?;
                let col_names: Vec<String> = stmt.column_names().iter().map(|s| (*s).to_string()).collect();
                let mut rows_out: Vec<Map<String, Value>> = Vec::new();
                let mut rows = stmt.query(params_from_iter(sqlite_params.clone()))?;
                let mut n = 0u32;
                while let Some(row) = rows.next()? {
                    if n >= max_rows {
                        break;
                    }
                    let mut map = Map::new();
                    for (i, name) in col_names.iter().enumerate() {
                        let v = row.get_ref(i)?;
                        map.insert(name.clone(), cell_to_json(v));
                    }
                    rows_out.push(map);
                    n += 1;
                }
                output_ok(
                    rows_out,
                    col_names,
                    n as usize,
                    started_total.elapsed().as_millis(),
                )
            };
            conn.progress_handler(0, None::<fn() -> bool>);
            Ok(out)
        })();

        match result {
            Ok(s) => return Ok(s),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                let busy = msg.contains("busy") || msg.contains("locked");
                if busy && attempt + 1 < BUSY_MAX_ATTEMPTS {
                    std::thread::sleep(Duration::from_millis(BUSY_BASE_MS * (attempt as u64 + 1)));
                    last_err = Some(e);
                    continue;
                }
                let elapsed = started_total.elapsed().as_millis();
                if msg.contains("interrupted") || e.to_string().to_lowercase().contains("interrupt") {
                    return Ok(output_err(format!(
                        "database_query timed out after {} seconds",
                        QUERY_TIMEOUT.as_secs()
                    )));
                }
                return Ok(json!({
                    "success": false,
                    "error": map_sqlite_error(e),
                    "execution_time_ms": elapsed,
                })
                .to_string());
            }
        }
    }
    Ok(json!({
        "success": false,
        "error": last_err.map(map_sqlite_error).unwrap_or_else(|| "Database busy after retries".into()),
        "execution_time_ms": started_total.elapsed().as_millis(),
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use std::path::PathBuf;

    fn tmp_workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("dbq-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn select_limit_works() {
        let ws = tmp_workspace();
        std::fs::create_dir_all(&ws).unwrap();
        let dbp = ws.join("t.db");
        {
            let c = Connection::open(&dbp).unwrap();
            c.execute_batch("CREATE TABLE conversations (id INTEGER, title TEXT); INSERT INTO conversations VALUES (1,'a'),(2,'b'),(3,'c');")
                .unwrap();
        }
        let rel = "t.db";
        let args = json!({
            "db_path": rel,
            "query": "SELECT * FROM conversations LIMIT 5",
        });
        let out = run_database_query_sync(Some(ws.as_path()), true, &ws, false, false, &args).unwrap();
        assert!(out.contains("\"success\":true"), "{out}");
        assert!(out.contains("\"title\":\"a\""), "{out}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn insert_blocked_readonly() {
        let ws = tmp_workspace();
        std::fs::create_dir_all(&ws).unwrap();
        let dbp = ws.join("w.db");
        {
            let c = Connection::open(&dbp).unwrap();
            c.execute_batch("CREATE TABLE test (x INTEGER);").unwrap();
        }
        let args = json!({
            "db_path": "w.db",
            "query": "INSERT INTO test VALUES (1)",
        });
        let out = run_database_query_sync(Some(ws.as_path()), true, &ws, false, false, &args).unwrap();
        assert!(out.contains("read-only") || out.contains("Write operation blocked"), "{out}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn insert_allowed_when_write_enabled() {
        let ws = tmp_workspace();
        std::fs::create_dir_all(&ws).unwrap();
        let dbp = ws.join("w2.db");
        {
            let c = Connection::open(&dbp).unwrap();
            c.execute_batch("CREATE TABLE test (x INTEGER);").unwrap();
        }
        let args = json!({
            "db_path": "w2.db",
            "query": "INSERT INTO test VALUES (?)",
            "params": [42],
        });
        let out = run_database_query_sync(Some(ws.as_path()), true, &ws, false, true, &args).unwrap();
        assert!(out.contains("\"success\":true"), "{out}");
        let verify = Connection::open(&dbp).unwrap();
        let n: i64 = verify
            .query_row("SELECT COUNT(*) FROM test WHERE x=42", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn drop_always_blocked() {
        let ws = tmp_workspace();
        std::fs::create_dir_all(&ws).unwrap();
        let dbp = ws.join("d.db");
        {
            let c = Connection::open(&dbp).unwrap();
            c.execute_batch("CREATE TABLE memories (x INT);").unwrap();
        }
        let args = json!({
            "db_path": "d.db",
            "query": "DROP TABLE memories",
        });
        let out = run_database_query_sync(Some(ws.as_path()), true, &ws, false, true, &args).unwrap();
        assert!(out.contains("blocked") || out.contains("DROP"), "{out}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn unknown_table_message() {
        let ws = tmp_workspace();
        std::fs::create_dir_all(&ws).unwrap();
        let dbp = ws.join("u.db");
        Connection::open(&dbp).unwrap();
        let args = json!({
            "db_path": "u.db",
            "query": "SELECT * FROM nonexistent",
        });
        let out = run_database_query_sync(Some(ws.as_path()), true, &ws, false, false, &args).unwrap();
        assert!(out.contains("not found") || out.contains("nonexistent"), "{out}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn meta_tables() {
        let ws = tmp_workspace();
        std::fs::create_dir_all(&ws).unwrap();
        let dbp = ws.join("m.sqlite");
        {
            let c = Connection::open(&dbp).unwrap();
            c.execute_batch("CREATE TABLE foo (a INT);").unwrap();
        }
        let args = json!({ "db_path": "m.sqlite", "query": ".tables" });
        let out = run_database_query_sync(Some(ws.as_path()), true, &ws, false, false, &args).unwrap();
        assert!(out.contains("foo"), "{out}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn app_data_location_reads_nova_style_db() {
        let dd = tmp_workspace();
        std::fs::create_dir_all(&dd).unwrap();
        let dbp = dd.join("nova_memory.sqlite");
        {
            let c = Connection::open(&dbp).unwrap();
            c.execute_batch("CREATE TABLE legacy (id INTEGER); INSERT INTO legacy VALUES (99);")
                .unwrap();
        }
        let args = json!({
            "location": "app_data",
            "db_path": "nova_memory.sqlite",
            "query": "SELECT * FROM legacy",
        });
        let out = run_database_query_sync(None, false, &dd, true, false, &args).unwrap();
        assert!(out.contains("\"success\":true"), "{out}");
        assert!(out.contains("99"), "{out}");
        let _ = std::fs::remove_dir_all(&dd);
    }
}
