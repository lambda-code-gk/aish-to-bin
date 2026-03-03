use serde::{Deserialize, Serialize};

/// Agent の外側ループ挙動
///
/// - Act: 実行寄り（押し込み有効）
/// - Plan: 計画・提案寄り（押し込み無効）
/// - Auto: 将来の Judge 等向け予約（v1.2 では Act と同等扱い）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Act,
    Plan,
    Auto,
}

impl AgentMode {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Act => "act",
            AgentMode::Plan => "plan",
            AgentMode::Auto => "auto",
        }
    }
}
