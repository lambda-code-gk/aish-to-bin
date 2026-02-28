//! セッション永続の一次ソース用イベントエンベロープ（events.ndjson スキーマ）
//!
//! v は破壊的変更で上げる。payload に巨大文字列を入れない（本文は artifacts に逃がす）。

use serde::{Deserialize, Serialize};

/// 1行1JSON で events.ndjson に追記する安定スキーマ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// スキーマバージョン（破壊的変更で増やす）
    pub v: u32,
    /// セッション内で単調増加する連番
    pub seq: u64,
    /// Unix ミリ秒（UTC）
    pub ts_ms: i64,
    /// セッション識別子（SessionDir 由来・既存ID形式に合わせる）
    pub session_id: String,
    /// AiRun / ToolCall などの相関用（あれば）
    pub run_id: Option<String>,
    /// 種別（例: "context.pack_built", "policy.evaluated", "run.failed"）
    pub kind: String,
    /// 任意ペイロード（巨大文字列は入れない）
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    pub const SCHEMA_VERSION: u32 = 1;
}
