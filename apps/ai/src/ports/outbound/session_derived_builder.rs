//! セッション派生物（index.sqlite / snapshots/summary.json）再生成の Outbound ポート

use crate::domain::EventEnvelope;
use common::domain::SessionDir;
use common::error::Error;

/// events から index / snapshots を再生成する能力
pub trait SessionDerivedBuilder: Send + Sync {
    /// 全イベントを消費して派生物を再生成する（v0.7 はバッチで既存を削除して作り直し）
    fn rebuild(
        &self,
        session_dir: &SessionDir,
        events: &mut dyn Iterator<Item = Result<EventEnvelope, Error>>,
    ) -> Result<(), Error>;
}
