//! reviewed 履歴の一覧・取得用 Outbound ポート

use crate::domain::{HistoryGetEntry, HistoryListEntry};
use common::error::Error;
use std::path::Path;

/// reviewed 履歴の一覧・取得（manifest.jsonl + reviewed/ を読む）
pub trait ReviewedHistoryReader: Send + Sync {
    /// 履歴一覧。all=true なら .history_send_from を無視して全件。user_only / assistant_only でロールで絞る。
    fn list_entries(
        &self,
        session_dir: &Path,
        all: bool,
        user_only: bool,
        assistant_only: bool,
    ) -> Result<Vec<HistoryListEntry>, Error>;

    /// 指定 id の内容を取得。存在しない id はスキップ（エラーにしない）。
    fn get_entries(
        &self,
        session_dir: &Path,
        ids: &[String],
    ) -> Result<Vec<HistoryGetEntry>, Error>;
}
