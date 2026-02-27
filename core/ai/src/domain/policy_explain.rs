use crate::domain::PolicyDecision;
use serde::{Deserialize, Serialize};

/// policy explain 全体
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyExplainInfo {
    pub v: u32,
    /// 解決済み policy 設定（defaults + env + config 等をマージした値）
    pub resolved: serde_json::Value,
    /// egress ルール名の順序
    pub egress_rules: Vec<String>,
    /// tool ルール名の順序
    pub tool_rules: Vec<String>,
    /// 代表的な評価例
    pub examples: Vec<PolicyExplainExample>,
}

/// explain 用の代表例
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyExplainExample {
    pub title: String,
    pub input: serde_json::Value,
    pub outcome: serde_json::Value,
    /// オプション: どのルールがヒットしたかなどの補足
    pub decisions: Vec<PolicyDecision>,
}

