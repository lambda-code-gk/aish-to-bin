//! events.jsonl 追記ストア（<session_dir>/events.jsonl）

use crate::domain::EventEnvelope;
use crate::ports::outbound::SessionEventStore;
use common::domain::SessionDir;
use common::error::Error;
use common::session_schema::require_latest;
use std::sync::Arc;

const EVENTS_FILE: &str = "events.jsonl";

/// 追記のみ。1行1JSON。seq は next_seq で採番してから append すること。
pub struct NdjsonSessionEventStore {
    fs: Arc<dyn common::ports::outbound::FileSystem>,
}

impl NdjsonSessionEventStore {
    pub fn new(fs: Arc<dyn common::ports::outbound::FileSystem>) -> Self {
        Self { fs }
    }

    fn events_path(session_dir: &SessionDir) -> std::path::PathBuf {
        session_dir.as_ref().join(EVENTS_FILE)
    }
}

impl SessionEventStore for NdjsonSessionEventStore {
    fn next_seq(&self, session_dir: &SessionDir) -> Result<u64, Error> {
        require_latest(self.fs.as_ref(), session_dir.as_ref())?;
        let path = Self::events_path(session_dir);
        let content = match self.fs.read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Ok(1), // ファイルが無い or 読めない → 1
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Ok(1);
        }
        let last_line = trimmed.lines().last().unwrap_or(trimmed);
        let last: EventEnvelope = serde_json::from_str(last_line).map_err(|e| {
            Error::json(format!(
                "events.jsonl last line parse failed: {}",
                e
            ))
        })?;
        Ok(last.seq.saturating_add(1))
    }

    fn append(&self, session_dir: &SessionDir, event: &EventEnvelope) -> Result<(), Error> {
        require_latest(self.fs.as_ref(), session_dir.as_ref())?;
        if event.seq == 0 {
            return Err(Error::invalid_argument(
                "EventEnvelope.seq must be assigned (use next_seq before append)",
            ));
        }
        let path = Self::events_path(session_dir);
        let line = serde_json::to_string(event).map_err(|e| Error::json(e.to_string()))?;
        let mut w = self.fs.open_append(&path)?;
        use std::io::Write;
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
        w.flush().map_err(|e| Error::io_msg(e.to_string()))?;
        Ok(())
    }

    fn read_all(
        &self,
        session_dir: &SessionDir,
    ) -> Result<
        Box<dyn Iterator<Item = Result<EventEnvelope, Error>> + Send>,
        Error,
    > {
        require_latest(self.fs.as_ref(), session_dir.as_ref())?;
        let path = Self::events_path(session_dir);
        let content = match self.fs.read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                // ファイルが無い場合は空イテレータ
                return Ok(Box::new(std::iter::empty()));
            }
        };
        let lines: Vec<String> = content.lines().map(String::from).collect();
        let iter = lines
            .into_iter()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return Err(Error::json("empty line in events.jsonl"));
                }
                serde_json::from_str(trimmed).map_err(|e| Error::json(e.to_string()))
            });
        Ok(Box::new(iter))
    }
}

impl common::ports::outbound::SessionEventStore for NdjsonSessionEventStore {
    fn next_seq(
        &self,
        session_dir: &common::domain::SessionDir,
    ) -> Result<u64, common::error::Error> {
        <Self as SessionEventStore>::next_seq(self, session_dir)
    }

    fn append(
        &self,
        session_dir: &common::domain::SessionDir,
        event: &common::domain::EventEnvelope,
    ) -> Result<(), common::error::Error> {
        <Self as SessionEventStore>::append(self, session_dir, event)
    }

    fn read_all(
        &self,
        session_dir: &common::domain::SessionDir,
    ) -> Result<
        Box<
            dyn Iterator<Item = Result<common::domain::EventEnvelope, common::error::Error>>
                + Send,
        >,
        common::error::Error,
    > {
        let inner = <Self as SessionEventStore>::read_all(self, session_dir)?;
        Ok(Box::new(inner))
    }
}
