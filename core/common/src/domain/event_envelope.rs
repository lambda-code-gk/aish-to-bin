//! セッション永続の一次ソース用イベントエンベロープ（events.jsonl スキーマ）
//!
//! v は破壊的変更で上げる。payload に巨大文字列を入れない（本文は artifacts に逃がす）。

use serde::{Deserialize, Serialize};

/// seq 未採番のイベントエンベロープ（append 入口用）
///
/// - `EventAppender` / daemon RPC の入力として使用する
/// - `seq` は常に 0 以外の値を実装側で採番し、`EventEnvelope` に変換してから永続する
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeWithoutSeq {
    /// スキーマバージョン（破壊的変更で増やす）
    pub v: u32,
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

/// 1行1JSON で events.jsonl に追記する安定スキーマ
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

    /// 推奨: payload の JSON バイト数がこれを超える場合は preview + artifact_rel_path に逃がす（P8-4）
    pub const RECOMMENDED_MAX_PAYLOAD_BYTES: usize = 4096;

    /// `EventEnvelopeWithoutSeq` に seq を付与して永続用エンベロープを生成する
    pub fn from_without_seq(seq: u64, base: EventEnvelopeWithoutSeq) -> Self {
        Self {
            v: base.v,
            seq,
            ts_ms: base.ts_ms,
            session_id: base.session_id,
            run_id: base.run_id,
            kind: base.kind,
            payload: base.payload,
        }
    }
}
