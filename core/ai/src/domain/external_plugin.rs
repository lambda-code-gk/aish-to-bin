//! 外部ツールプラグインのドメイン型（manifest / descriptor / error）
//!
//! AISH本体は個別ツール名を知らず、プラグインから取得した定義を動的登録する。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// 外部プラグイン識別子（newtype）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalPluginId(pub String);

impl ExternalPluginId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for ExternalPluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// トランスポート種別（将来 MCP / HTTP 等に拡張）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginTransportType {
    Stdio,
}

/// Stdio トランスポート設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdioTransport {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// プラグインのタイムアウト設定（ミリ秒）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginTimeouts {
    /// 起動完了までのタイムアウト
    pub startup_ms: Option<u64>,
    /// call_tool 1回あたりのタイムアウト
    pub call_ms: Option<u64>,
}

/// プラグイン manifest（YAML/JSON から読み込む）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub transport: PluginTransport,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub timeouts: PluginTimeouts,
    /// 無効時はスキップ（default: true）
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// トランスポート設定（MVP は stdio のみ）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginTransport {
    Stdio(StdioTransport),
}

/// 外部ツールの定義（list_tools の 1 件）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalToolDescriptor {
    pub name: String,
    pub description: String,
    /// JSON Schema（LLM 用 parameters にそのまま渡す）
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

/// 外部ツール呼び出しリクエスト（call_tool の params）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalToolCallRequest {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// 外部ツール呼び出しレスポンス（call_tool の result）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalToolCallResponse {
    #[serde(default)]
    pub content: serde_json::Value,
}

/// 外部プラグイン関連エラー（fail-closed でプラグインのみ無効化する際に利用）
#[derive(Debug, Clone, thiserror::Error)]
pub enum ExternalPluginError {
    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),
    #[error("Plugin start failed: {0}")]
    StartFailed(String),
    #[error("List tools failed: {0}")]
    ListToolsFailed(String),
    #[error("Tool call failed: {0}")]
    ToolCallFailed(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Malformed response: {0}")]
    MalformedResponse(String),
    #[error("Plugin process exited: {0}")]
    ProcessExited(String),
}

/// Discovery 結果 1 件（manifest と読み込み元パス）
#[derive(Debug, Clone)]
pub struct PluginManifestEntry {
    pub manifest: PluginManifest,
    pub manifest_path: PathBuf,
}
