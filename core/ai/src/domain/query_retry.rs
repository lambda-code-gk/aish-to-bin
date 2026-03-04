use serde::{Deserialize, Serialize};

/// Agent の外側ループの Query retry 方針
///
/// - Act: 実行寄り（押し込み有効・デフォルト）
/// - Plan: 計画・提案寄り（押し込み無効）
/// - Auto: CompositeJudge による自動判定（LLM Judge を含む）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryRetry {
    Act,
    Plan,
    Auto,
}

impl QueryRetry {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueryRetry::Act => "act",
            QueryRetry::Plan => "plan",
            QueryRetry::Auto => "auto",
        }
    }
}
