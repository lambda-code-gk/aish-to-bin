//! セッションイベント永続の Outbound ポート（events.ndjson 追記・単一ライタ）
//!
//! P8-3: 追記はこの trait 経由のみ。capture/orchestrator は fs 直書き禁止。

use crate::domain::{EventEnvelope, SessionDir};
use crate::error::Error;

/// セッション内イベントを追記のみで永続する能力（唯一の追記口）
pub trait SessionEventStore: Send + Sync {
    /// 次の seq を採番する（append 前に呼ぶ）
    fn next_seq(&self, session_dir: &SessionDir) -> Result<u64, Error>;

    /// 1件追記する。event.seq は 0 でないこと（next_seq で採番済みであること）
    fn append(&self, session_dir: &SessionDir, event: &EventEnvelope) -> Result<(), Error>;

    /// 全イベントを先頭から読む。パース失敗した行は Err を yield する
    fn read_all(
        &self,
        session_dir: &SessionDir,
    ) -> Result<Box<dyn Iterator<Item = Result<EventEnvelope, Error>> + Send>, Error>;
}
