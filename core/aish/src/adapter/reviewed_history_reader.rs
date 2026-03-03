//! reviewed 履歴の一覧・取得アダプタ（manifest.jsonl + reviewed/ を読む）

use crate::domain::{HistoryGetEntry, HistoryListEntry};
use crate::ports::outbound::ReviewedHistoryReader;
use common::error::Error;
use common::ports::outbound::FileSystem;
use common::safe_session_path::{
    is_safe_reviewed_path, resolve_under_session_dir, HISTORY_SEND_FROM_FILENAME,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// manifest.jsonl の message 行の最小パース用（aish は ai に依存しない）
#[derive(serde::Deserialize)]
struct ManifestMessageLine {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    ts: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    reviewed_path: String,
}

fn manifest_path(session_dir: &Path) -> PathBuf {
    session_dir.join("manifest.jsonl")
}

fn send_from_path(session_dir: &Path) -> PathBuf {
    session_dir.join(HISTORY_SEND_FROM_FILENAME)
}

fn load_send_from_index(fs: &dyn FileSystem, session_dir: &Path) -> usize {
    let path = send_from_path(session_dir);
    if !fs.exists(&path) {
        return 0;
    }
    let s = match fs.read_to_string(&path) {
        Ok(x) => x,
        Err(_) => return 0,
    };
    s.trim().parse::<usize>().unwrap_or(0)
}

/// 先頭1行を取得（改行まで、末尾改行は除く）
fn first_line(s: &str) -> &str {
    let trimmed = s.trim_end_matches('\n');
    trimmed.lines().next().unwrap_or("").trim_end()
}

/// manifest の ts を "YYYY-mm-dd HH:MM" に整形（例: "2026-02-22T12:00:00" → "2026-02-22 12:00"）
fn format_datetime(ts: &str) -> String {
    let ts = ts.trim();
    if let Some((date, time)) = ts.split_once('T') {
        let time_part: String = time.chars().take(5).collect();
        format!("{} {}", date, time_part)
    } else {
        ts.chars().take(16).collect::<String>()
    }
}

/// 先頭行を max_chars 文字で切り詰め、超えたら "..." を付ける
fn truncate_first_line(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let first: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", first)
    } else {
        first
    }
}

pub(crate) struct StdReviewedHistoryReader {
    fs: Arc<dyn FileSystem>,
}

impl StdReviewedHistoryReader {
    pub(crate) fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    fn load_message_lines(&self, session_dir: &Path) -> Result<Vec<ManifestMessageLine>, Error> {
        let path = manifest_path(session_dir);
        if !self.fs.exists(&path) {
            return Ok(Vec::new());
        }
        let s = self.fs.read_to_string(&path)?;
        let mut out = Vec::new();
        for line in s.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(rec) = serde_json::from_str::<ManifestMessageLine>(trimmed) {
                if rec.kind == "message"
                    && !rec.id.is_empty()
                    && is_safe_reviewed_path(&rec.reviewed_path)
                {
                    out.push(rec);
                }
            }
        }
        Ok(out)
    }

    fn read_reviewed_content(
        &self,
        session_dir: &Path,
        reviewed_path: &str,
    ) -> Result<String, Error> {
        let full = session_dir.join(reviewed_path);
        let safe = resolve_under_session_dir(session_dir, &full).ok_or_else(|| {
            Error::invalid_argument(format!(
                "reviewed_path not under session dir: {}",
                reviewed_path
            ))
        })?;
        self.fs.read_to_string(&safe)
    }
}

/// 一覧用の先頭行はここでは切らず、main のターミナル幅に任せる（十分な長さを渡す）
const FIRST_LINE_MAX_CHARS: usize = 500;

impl ReviewedHistoryReader for StdReviewedHistoryReader {
    fn list_entries(
        &self,
        session_dir: &Path,
        all: bool,
        user_only: bool,
        assistant_only: bool,
    ) -> Result<Vec<HistoryListEntry>, Error> {
        let lines = self.load_message_lines(session_dir)?;
        let send_from = if all {
            0
        } else {
            load_send_from_index(self.fs.as_ref(), session_dir)
        };
        let mut result = Vec::new();
        for (index, rec) in lines.iter().enumerate() {
            if index < send_from {
                continue;
            }
            let role_ok = (!user_only && !assistant_only)
                || (user_only && rec.role == "user")
                || (assistant_only && rec.role == "assistant");
            if !role_ok {
                continue;
            }
            let content = match self.read_reviewed_content(session_dir, &rec.reviewed_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let raw_first = first_line(&content);
            result.push(HistoryListEntry {
                id: rec.id.clone(),
                datetime: format_datetime(&rec.ts),
                first_line: truncate_first_line(raw_first, FIRST_LINE_MAX_CHARS),
            });
        }
        Ok(result)
    }

    fn get_entries(
        &self,
        session_dir: &Path,
        ids: &[String],
    ) -> Result<Vec<HistoryGetEntry>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let id_set: std::collections::HashSet<_> = ids.iter().map(|s| s.as_str()).collect();
        let lines = self.load_message_lines(session_dir)?;
        let mut result = Vec::new();
        for rec in lines {
            if !id_set.contains(rec.id.as_str()) {
                continue;
            }
            let content = match self.read_reviewed_content(session_dir, &rec.reviewed_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            result.push(HistoryGetEntry {
                id: rec.id.clone(),
                content,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;

    #[test]
    fn test_format_datetime() {
        assert_eq!(format_datetime("2026-02-22T12:00:00"), "2026-02-22 12:00");
        assert_eq!(format_datetime("2026-02-22T09:05"), "2026-02-22 09:05");
    }

    #[test]
    fn test_truncate_first_line() {
        assert_eq!(truncate_first_line("short", 20), "short");
        assert_eq!(
            truncate_first_line("12345678901234567890", 20),
            "12345678901234567890"
        );
        assert_eq!(
            truncate_first_line("12345678901234567890123", 20),
            "12345678901234567890..."
        );
    }

    #[test]
    fn test_list_entries_empty_manifest() {
        let temp = std::env::temp_dir().join(format!("aish_hist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join("manifest.jsonl"), "").unwrap();
        let reader = StdReviewedHistoryReader::new(Arc::new(StdFileSystem));
        let entries = reader.list_entries(&temp, true, false, false).unwrap();
        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_list_entries_and_get() {
        let temp = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("aish_hist_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let reviewed_dir = temp.join("reviewed");
        std::fs::create_dir_all(&reviewed_dir).unwrap();
        std::fs::write(
            reviewed_dir.join("reviewed_001_user.txt"),
            "First user line\nsecond",
        )
        .unwrap();
        std::fs::write(
            reviewed_dir.join("reviewed_002_assistant.txt"),
            "Assistant reply",
        )
        .unwrap();
        let manifest = r#"{"kind":"message","v":1,"ts":"2026-02-22T12:00:00","id":"001","role":"user","part_path":"part_001_user.txt","reviewed_path":"reviewed/reviewed_001_user.txt","decision":"allow","bytes":1,"hash64":"a"}
{"kind":"message","v":1,"ts":"2026-02-22T12:01:00","id":"002","role":"assistant","part_path":"part_002_assistant.txt","reviewed_path":"reviewed/reviewed_002_assistant.txt","decision":"allow","bytes":1,"hash64":"b"}
"#;
        std::fs::write(temp.join("manifest.jsonl"), manifest).unwrap();
        let session_dir = temp.canonicalize().unwrap();
        let reader = StdReviewedHistoryReader::new(Arc::new(StdFileSystem));
        let entries = reader
            .list_entries(&session_dir, true, false, false)
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "001");
        assert_eq!(entries[0].datetime, "2026-02-22 12:00");
        assert_eq!(entries[0].first_line, "First user line");
        assert_eq!(entries[1].id, "002");
        assert_eq!(entries[1].first_line, "Assistant reply");
        let user_only = reader
            .list_entries(&session_dir, true, true, false)
            .unwrap();
        assert_eq!(user_only.len(), 1);
        assert_eq!(user_only[0].id, "001");
        let assistant_only = reader
            .list_entries(&session_dir, true, false, true)
            .unwrap();
        assert_eq!(assistant_only.len(), 1);
        assert_eq!(assistant_only[0].id, "002");
        let get_entries = reader
            .get_entries(&session_dir, &["002".to_string(), "001".to_string()])
            .unwrap();
        assert_eq!(get_entries.len(), 2);
        // 返却順は manifest の並び順（001, 002）
        assert_eq!(get_entries[0].id, "001");
        assert_eq!(get_entries[0].content, "First user line\nsecond");
        assert_eq!(get_entries[1].id, "002");
        assert_eq!(get_entries[1].content, "Assistant reply");
        let _ = std::fs::remove_dir_all(&temp);
    }
}
