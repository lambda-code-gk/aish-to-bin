//! セッション派生物: index.sqlite / snapshots/summary.json
//!
//! Phase 10.1: DerivedApplier（増分適用）と DerivedRebuilder（全再構築）を提供。
//! CLI と daemon の両方から同じ API を呼ぶ。

use common::domain::{EventEnvelope, SessionDir};
use common::error::Error;
use common::ports::outbound::{FileSystem, SessionEventStore};
use rusqlite::params;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

const INDEX_DIR: &str = "index";
const INDEX_DB: &str = "index.sqlite";
const SNAPSHOTS_DIR: &str = "snapshots";
const SUMMARY_JSON: &str = "summary.json";
const METADATA_TABLE: &str =
    "CREATE TABLE IF NOT EXISTS metadata (k TEXT PRIMARY KEY, v INTEGER NOT NULL);";
const LAST_APPLIED_SEQ_KEY: &str = "last_applied_seq";

const PREVIEW_MAX: usize = 200;

/// 最大 max バイトで切り詰め、末尾を "..." にする。UTF-8 の文字境界で切る。
fn truncate_str(s: &str, max: usize) -> String {
    let s = s.trim();
    let limit = max.saturating_sub(3);
    if s.len() <= limit {
        return s.to_string();
    }
    let mut end = limit.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

fn session_id_from_dir(session_dir: &SessionDir) -> String {
    session_dir
        .as_ref()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn subject_and_text_from_payload(kind: &str, payload: &serde_json::Value) -> (String, String) {
    match kind {
        "policy.evaluated" => {
            let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let reason = payload.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            let subject = format!("{} {}", status, reason).trim().to_string();
            let text = payload
                .get("details")
                .and_then(|d| d.get("command").and_then(|c| c.as_str()))
                .or_else(|| payload.get("tool_name").and_then(|t| t.as_str()))
                .unwrap_or("")
                .to_string();
            (subject, truncate_str(&text, 200))
        }
        "context.pack_built" => {
            let addons = payload
                .get("addons_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let attachments = payload
                .get("attachments_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            (
                "context pack built".to_string(),
                format!("addons={} attachments={}", addons, attachments),
            )
        }
        k if k.starts_with("tool.") => {
            let name = payload
                .get("tool_name")
                .or(payload.get("tool"))
                .and_then(|v| v.as_str())
                .unwrap_or(k);
            let subject = format!("tool {}", name);
            let args_preview = payload
                .get("args")
                .map(|a| serde_json::to_string(a).unwrap_or_default())
                .unwrap_or_default();
            (subject, truncate_str(&args_preview, 200))
        }
        _ => (kind.to_string(), String::new()),
    }
}

/// 増分適用の結果（適用した seq 範囲）
#[derive(Debug, Clone)]
pub struct AppliedRange {
    pub from_seq: u64,
    pub to_seq: u64,
}

/// 増分適用: from_seq 以降のイベントを index/snapshots に反映する。
/// index が無い場合は作成し、metadata に last_applied_seq を記録する。
pub struct DerivedApplier {
    store: Arc<dyn SessionEventStore>,
    fs: Arc<dyn FileSystem>,
}

impl DerivedApplier {
    pub fn new(store: Arc<dyn SessionEventStore>, fs: Arc<dyn FileSystem>) -> Self {
        Self { store, fs }
    }

    fn resolve_payload(
        &self,
        ev: &EventEnvelope,
        session_dir: &SessionDir,
    ) -> Result<serde_json::Value, Error> {
        let path = match ev.payload.get("artifact_rel_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return Ok(ev.payload.clone()),
        };
        let full = session_dir.as_ref().join(path);
        let s = self
            .fs
            .read_to_string(&full)
            .map_err(|e| Error::io_msg(format!("read event artifact {}: {}", path, e)))?;
        serde_json::from_str(&s)
            .map_err(|e| Error::json(format!("parse event artifact {}: {}", path, e)))
    }

    /// from_seq 以降のイベントを読み、index と snapshots に反映する。
    /// 適用した seq 範囲を返す。適用するイベントが無ければ to_seq < from_seq で返す。
    pub fn apply_from_seq(
        &self,
        session_dir: &SessionDir,
        from_seq: u64,
    ) -> Result<AppliedRange, Error> {
        let base = session_dir.as_ref();
        let index_dir = base.join(INDEX_DIR);
        let index_path = index_dir.join(INDEX_DB);
        let snapshots_dir = base.join(SNAPSHOTS_DIR);
        let summary_path = snapshots_dir.join(SUMMARY_JSON);

        let mut to_apply: Vec<EventEnvelope> = Vec::new();
        let mut iter = self.store.read_all(session_dir)?;
        for item in iter.by_ref() {
            let ev = item?;
            if ev.seq >= from_seq {
                to_apply.push(ev);
            }
        }
        let to_seq = to_apply.last().map(|e| e.seq).unwrap_or(0);
        if to_apply.is_empty() {
            return Ok(AppliedRange {
                from_seq,
                to_seq: from_seq.saturating_sub(1),
            });
        }

        let mut resolved: Vec<(EventEnvelope, serde_json::Value)> =
            Vec::with_capacity(to_apply.len());
        for ev in &to_apply {
            let r = self.resolve_payload(ev, session_dir)?;
            resolved.push((ev.clone(), r));
        }

        let need_init = !index_path.exists();
        if need_init {
            fs::create_dir_all(&index_dir)
                .map_err(|e| Error::io_msg(format!("create_dir_all index: {}", e)))?;
            let conn = rusqlite::Connection::open(&index_path)
                .map_err(|e| Error::io_msg(format!("open index.sqlite: {}", e)))?;
            let sql = format!(
                r#"
                CREATE TABLE events (
                    seq INTEGER PRIMARY KEY,
                    ts_ms INTEGER NOT NULL,
                    kind TEXT NOT NULL,
                    run_id TEXT,
                    subject TEXT,
                    text TEXT,
                    payload_json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_events_ts_ms ON events(ts_ms);
                CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
                CREATE INDEX IF NOT EXISTS idx_events_run_id ON events(run_id);
                {}
                "#,
                METADATA_TABLE
            );
            conn.execute_batch(&sql)
                .map_err(|e| Error::io_msg(format!("create table: {}", e)))?;
        }

        let conn = rusqlite::Connection::open(&index_path)
            .map_err(|e| Error::io_msg(format!("open index.sqlite: {}", e)))?;
        let mut insert = conn
            .prepare(
                "INSERT OR REPLACE INTO events (seq, ts_ms, kind, run_id, subject, text, payload_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| Error::io_msg(format!("prepare insert: {}", e)))?;
        for (ev, resolved_val) in &resolved {
            let (subject, text) = subject_and_text_from_payload(ev.kind.as_str(), resolved_val);
            let payload_json =
                serde_json::to_string(resolved_val).unwrap_or_else(|_| "{}".to_string());
            insert
                .execute(params![
                    ev.seq as i64,
                    ev.ts_ms,
                    &ev.kind,
                    ev.run_id.as_deref(),
                    subject,
                    text,
                    payload_json,
                ])
                .map_err(|e| Error::io_msg(format!("insert event: {}", e)))?;
        }
        drop(insert);

        conn.execute(
            "INSERT OR REPLACE INTO metadata (k, v) VALUES (?1, ?2)",
            params![LAST_APPLIED_SEQ_KEY, to_seq as i64],
        )
        .map_err(|e| Error::io_msg(format!("update metadata: {}", e)))?;
        drop(conn);

        self.write_summary(session_dir, &index_path, &summary_path, &snapshots_dir)?;

        Ok(AppliedRange { from_seq, to_seq })
    }

    fn write_summary(
        &self,
        session_dir: &SessionDir,
        index_path: &Path,
        summary_path: &Path,
        snapshots_dir: &Path,
    ) -> Result<(), Error> {
        let conn = rusqlite::Connection::open(index_path)
            .map_err(|e| Error::io_msg(format!("open index for summary: {}", e)))?;
        let mut stmt = conn
            .prepare(
                "SELECT seq, ts_ms, kind, subject, text, payload_json FROM events ORDER BY seq",
            )
            .map_err(|e| Error::io_msg(format!("prepare: {}", e)))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| Error::io_msg(format!("query: {}", e)))?;

        let mut counts_by_kind: HashMap<String, u64> = HashMap::new();
        let mut blocked_count: u64 = 0;
        let mut warn_count: u64 = 0;
        let mut last_ts_ms: Option<i64> = None;
        let mut last_user_query_preview: Option<String> = None;
        let mut last_assistant_text_preview: Option<String> = None;
        let mut last_tool_subjects: Vec<String> = Vec::new();

        for row in rows {
            let (seq, ts_ms, kind, subject, _text, payload_json): (
                i64,
                i64,
                String,
                String,
                String,
                String,
            ) = row.map_err(|e| Error::io_msg(e.to_string()))?;
            let _ = seq;
            *counts_by_kind.entry(kind.clone()).or_insert(0) += 1;
            last_ts_ms = Some(ts_ms);
            if kind == "policy.evaluated" {
                let payload: serde_json::Value =
                    serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
                let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status == "blocked" {
                    blocked_count += 1;
                } else if status == "warn" || status.contains("warning") {
                    warn_count += 1;
                }
            }
            if kind == "context.pack_built" || kind.starts_with("message.") {
                let payload: serde_json::Value =
                    serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
                if let Some(content) = payload.get("content").and_then(|c| c.as_str()) {
                    let preview = truncate_str(content, PREVIEW_MAX);
                    if kind.contains("user") {
                        last_user_query_preview = Some(preview);
                    } else if kind.contains("assistant") {
                        last_assistant_text_preview = Some(preview);
                    }
                }
            }
            if kind.starts_with("tool.") {
                last_tool_subjects.push(subject);
            }
        }
        if last_tool_subjects.len() > 20 {
            last_tool_subjects = last_tool_subjects[last_tool_subjects.len() - 20..].to_vec();
        }

        let generated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let session_id = session_id_from_dir(session_dir);
        let summary = serde_json::json!({
            "v": 1,
            "session_id": session_id,
            "generated_at_ms": generated_at_ms,
            "counts_by_kind": counts_by_kind,
            "last_ts_ms": last_ts_ms,
            "last_user_query_preview": last_user_query_preview,
            "last_assistant_text_preview": last_assistant_text_preview,
            "last_tool_subjects": last_tool_subjects,
            "blocked_count": blocked_count,
            "warn_count": warn_count,
        });
        let summary_str =
            serde_json::to_string_pretty(&summary).map_err(|e| Error::json(e.to_string()))?;
        fs::create_dir_all(snapshots_dir)
            .map_err(|e| Error::io_msg(format!("create_dir_all snapshots: {}", e)))?;
        self.fs.write(summary_path, &summary_str)?;
        Ok(())
    }
}

/// 全再構築: 全イベントを読み直し index と snapshots を作り直す。
pub struct DerivedRebuilder {
    store: Arc<dyn SessionEventStore>,
    fs: Arc<dyn FileSystem>,
}

impl DerivedRebuilder {
    pub fn new(store: Arc<dyn SessionEventStore>, fs: Arc<dyn FileSystem>) -> Self {
        Self { store, fs }
    }

    fn resolve_payload(
        &self,
        ev: &EventEnvelope,
        session_dir: &SessionDir,
    ) -> Result<serde_json::Value, Error> {
        let path = match ev.payload.get("artifact_rel_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return Ok(ev.payload.clone()),
        };
        let full = session_dir.as_ref().join(path);
        let s = self
            .fs
            .read_to_string(&full)
            .map_err(|e| Error::io_msg(format!("read event artifact {}: {}", path, e)))?;
        serde_json::from_str(&s)
            .map_err(|e| Error::json(format!("parse event artifact {}: {}", path, e)))
    }

    /// 全イベントを消費して index と summary を再生成する。
    pub fn rebuild_all(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let base = session_dir.as_ref();
        let index_dir = base.join(INDEX_DIR);
        let index_path = index_dir.join(INDEX_DB);
        let snapshots_dir = base.join(SNAPSHOTS_DIR);
        let summary_path = snapshots_dir.join(SUMMARY_JSON);

        let mut list: Vec<EventEnvelope> = Vec::new();
        let mut iter = self.store.read_all(session_dir)?;
        for item in iter.by_ref() {
            list.push(item?);
        }

        if index_path.exists() {
            fs::remove_file(&index_path)
                .map_err(|e| Error::io_msg(format!("remove index.sqlite: {}", e)))?;
        }
        fs::create_dir_all(&index_dir)
            .map_err(|e| Error::io_msg(format!("create_dir_all index: {}", e)))?;
        let conn = rusqlite::Connection::open(&index_path)
            .map_err(|e| Error::io_msg(format!("open index.sqlite: {}", e)))?;
        let sql = format!(
            r#"
            CREATE TABLE events (
                seq INTEGER PRIMARY KEY,
                ts_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                run_id TEXT,
                subject TEXT,
                text TEXT,
                payload_json TEXT NOT NULL
            );
            CREATE INDEX idx_events_ts_ms ON events(ts_ms);
            CREATE INDEX idx_events_kind ON events(kind);
            CREATE INDEX idx_events_run_id ON events(run_id);
            {}
            "#,
            METADATA_TABLE
        );
        conn.execute_batch(&sql)
            .map_err(|e| Error::io_msg(format!("create table: {}", e)))?;

        let mut insert = conn
            .prepare(
                "INSERT INTO events (seq, ts_ms, kind, run_id, subject, text, payload_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(|e| Error::io_msg(format!("prepare insert: {}", e)))?;
        let mut resolved_list: Vec<(EventEnvelope, serde_json::Value)> =
            Vec::with_capacity(list.len());
        for ev in &list {
            let resolved = self.resolve_payload(ev, session_dir)?;
            resolved_list.push((ev.clone(), resolved));
        }
        let last_seq = list.last().map(|e| e.seq).unwrap_or(0);
        for (ev, resolved) in &resolved_list {
            let (subject, text) = subject_and_text_from_payload(ev.kind.as_str(), resolved);
            let payload_json = serde_json::to_string(resolved).unwrap_or_else(|_| "{}".to_string());
            insert
                .execute(params![
                    ev.seq as i64,
                    ev.ts_ms,
                    &ev.kind,
                    ev.run_id.as_deref(),
                    subject,
                    text,
                    payload_json,
                ])
                .map_err(|e| Error::io_msg(format!("insert event: {}", e)))?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO metadata (k, v) VALUES (?1, ?2)",
            params![LAST_APPLIED_SEQ_KEY, last_seq as i64],
        )
        .map_err(|e| Error::io_msg(format!("update metadata: {}", e)))?;
        drop(insert);
        drop(conn);

        self.write_summary_from_list(session_dir, &resolved_list, &summary_path, &snapshots_dir)?;
        Ok(())
    }

    fn write_summary_from_list(
        &self,
        session_dir: &SessionDir,
        resolved_list: &[(EventEnvelope, serde_json::Value)],
        summary_path: &Path,
        snapshots_dir: &Path,
    ) -> Result<(), Error> {
        let mut counts_by_kind: HashMap<String, u64> = HashMap::new();
        let mut blocked_count: u64 = 0;
        let mut warn_count: u64 = 0;
        let mut last_ts_ms: Option<i64> = None;
        let mut last_user_query_preview: Option<String> = None;
        let mut last_assistant_text_preview: Option<String> = None;
        let mut last_tool_subjects: Vec<String> = Vec::new();

        for (ev, resolved) in resolved_list {
            *counts_by_kind.entry(ev.kind.clone()).or_insert(0) += 1;
            last_ts_ms = Some(ev.ts_ms);
            if ev.kind == "policy.evaluated" {
                let status = resolved
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if status == "blocked" {
                    blocked_count += 1;
                } else if status == "warn" || status.contains("warning") {
                    warn_count += 1;
                }
            }
            if ev.kind == "context.pack_built" || ev.kind.starts_with("message.") {
                if let Some(content) = resolved.get("content").and_then(|c| c.as_str()) {
                    let preview = truncate_str(content, PREVIEW_MAX);
                    if ev.kind.contains("user") {
                        last_user_query_preview = Some(preview);
                    } else if ev.kind.contains("assistant") {
                        last_assistant_text_preview = Some(preview);
                    }
                }
            }
            if ev.kind.starts_with("tool.") {
                let (subj, _) = subject_and_text_from_payload(ev.kind.as_str(), resolved);
                last_tool_subjects.push(subj);
            }
        }
        if last_tool_subjects.len() > 20 {
            last_tool_subjects = last_tool_subjects[last_tool_subjects.len() - 20..].to_vec();
        }

        let generated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let session_id = session_id_from_dir(session_dir);
        let summary = serde_json::json!({
            "v": 1,
            "session_id": session_id,
            "generated_at_ms": generated_at_ms,
            "counts_by_kind": counts_by_kind,
            "last_ts_ms": last_ts_ms,
            "last_user_query_preview": last_user_query_preview,
            "last_assistant_text_preview": last_assistant_text_preview,
            "last_tool_subjects": last_tool_subjects,
            "blocked_count": blocked_count,
            "warn_count": warn_count,
        });
        let summary_str =
            serde_json::to_string_pretty(&summary).map_err(|e| Error::json(e.to_string()))?;
        fs::create_dir_all(snapshots_dir)
            .map_err(|e| Error::io_msg(format!("create_dir_all snapshots: {}", e)))?;
        self.fs.write(summary_path, &summary_str)?;
        Ok(())
    }
}

/// index の metadata から last_applied_seq を読む。無ければ 0。
pub fn read_last_applied_seq(session_dir: &SessionDir) -> u64 {
    let path = session_dir.as_ref().join(INDEX_DIR).join(INDEX_DB);
    let conn = match rusqlite::Connection::open(&path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let v: i64 = conn
        .query_row(
            "SELECT v FROM metadata WHERE k = ?1",
            rusqlite::params![LAST_APPLIED_SEQ_KEY],
            |row| row.get(0),
        )
        .unwrap_or(0);
    v as u64
}
