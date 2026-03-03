use crate::domain::ToolCapability;
use serde::{Deserialize, Serialize};

/// ツールごとのポリシープロファイル
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolProfile {
    pub tool_name: String,
    pub mode: ToolMode,
    pub capabilities: Vec<ToolCapability>,
    /// 人間向けの補足説明（explain 出力用）
    pub notes: Option<String>,
}

/// ツールの基本モード
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolMode {
    Allow,
    RequireApproval,
    Deny,
}
