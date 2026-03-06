use crate::domain::ConfigSource;
use serde::{Deserialize, Serialize};

/// config explain 全体
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfigExplainInfo {
    pub v: u32,
    /// 解決済み設定ツリー（PolicyConfig などを serde_json::Value にしたもの）
    pub resolved: serde_json::Value,
    /// キーごとの最終値と出典
    pub sources: Vec<ConfigKeySource>,
    /// 追加メモ（優先順位やスキーマなど）
    pub notes: Vec<String>,
}

/// 各設定キーの出典情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfigKeySource {
    /// 設定キー（例: \"policy.egress_sensitive_action\")
    pub key: String,
    /// 値の短縮表示（human friendly）
    pub value_preview: String,
    pub source: ConfigSource,
}
