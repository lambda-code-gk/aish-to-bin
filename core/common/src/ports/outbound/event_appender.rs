//! セッションイベント append の唯一入口（P10-1 EventAppender）
//!
//! - capture / orchestrator / usecase 側はこの port のみを通じて events.ndjson に追記する
//! - seq の採番と実ストアへの書き込みは実装側（ローカルストア / daemon 経由）が担う

use crate::domain::{EventEnvelope, EventEnvelopeWithoutSeq, SessionDir};
use crate::error::Error;

/// セッション内イベントを単一入口から追記するためのポート
pub trait EventAppender: Send + Sync {
    /// seq 未採番のエンベロープを受け取り、seq を採番して永続し、付与済みエンベロープを返す
    fn append(
        &self,
        session_dir: &SessionDir,
        envelope: EventEnvelopeWithoutSeq,
    ) -> Result<EventEnvelope, Error>;
}

