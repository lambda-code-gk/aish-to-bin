use serde::{Deserialize, Serialize};

/// Agent の外側ループ挙動
///
/// - Act: 実行寄り（押し込み有効・デフォルト）
/// - Plan: 計画・提案寄り（押し込み無効）
/// - Auto: CompositeJudge による自動モード選択（LLM Judge を含む）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Act,
    Plan,
    Auto,
}

impl AgentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Act => "act",
            AgentMode::Plan => "plan",
            AgentMode::Auto => "auto",
        }
    }
}
