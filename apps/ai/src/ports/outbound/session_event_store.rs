//! セッションイベント永続の Outbound ポート（events.ndjson 追記）

use crate::domain::EventEnvelope;
use common::domain::SessionDir;
use common::error::Error;

/// セッション内イベントを追記のみで永続する能力
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
