//! MCP host / 外部ツール拡張の統一ポート
//!
//! Phase9 の狙い: 外部ツール連携はこの trait 経由に集約し、具体的な stdio JSON-RPC 等は adapter 側へ隔離する。

use crate::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 外部サーバ（プラグイン）を識別する ID（安定・一意）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct McpServerId(pub String);

impl McpServerId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for McpServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 外部ツール（canonical）を識別する ID（例: `namespace.tool`）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct McpToolId(pub String);

impl McpToolId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for McpToolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `discover()` が返すサーバ（プラグイン）の最小メタデータ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDescriptor {
    pub id: McpServerId,
    /// canonical tool id の namespace（例: `acme`）
    pub namespace: String,
    pub display_name: String,
    /// 明示 enable されているか（deny-by-default の状態を反映）
    pub enabled: bool,
    /// 任意: 設定ファイルの相対/絶対パス（CLI 表示用）
    #[serde(default)]
    pub source: Option<String>,
}

/// ツールの最小定義（LLM に渡す schema 用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// canonical id: `namespace.tool`
    pub id: McpToolId,
    pub display_name: String,
    /// 入力 JSON Schema（parameters）
    pub schema: Value,
    /// 任意: policy 判断補助（read/write/network 等）
    #[serde(default)]
    pub capabilities_hint: Vec<String>,
}

/// tool call の実行コンテキスト（最小）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpCallContext {
    /// タイムアウト（ms）。None の場合は host 側のデフォルトを適用。
    pub timeout_ms: Option<u64>,
    /// 監査/相関用（events 記録や host 側ログに利用）
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub non_interactive: bool,
}

/// tool call の結果（本文は Value、巨大 payload は上位で artifacts 化するのが想定）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCallResult {
    pub content: Value,
    /// 任意: stderr 末尾（cap 済み）
    #[serde(default)]
    pub stderr_tail: Option<String>,
    /// 任意: 実行時間（ms）
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
}

/// 外部拡張の唯一の口（MCP host 互換）
pub trait McpHost: Send + Sync {
    /// 信頼ディレクトリから discovery。enabled/disabled を含めた状態を返す。
    fn discover(&self) -> Result<Vec<McpServerDescriptor>, Error>;

    /// サーバ（プラグイン）内のツール一覧を返す。disabled なら fail-closed で Err。
    fn list_tools(&self, server_id: &McpServerId) -> Result<Vec<ToolDescriptor>, Error>;

    /// ツールを呼び出す。disabled / 未発見 / 不正応答等はすべて fail-closed（Err）。
    fn call(&self, tool_id: &McpToolId, args: Value, ctx: McpCallContext) -> Result<McpCallResult, Error>;

    /// 任意のヘルスチェック（未実装なら常に Ok）
    fn health(&self, _server_id: &McpServerId) -> Result<(), Error> {
        Ok(())
    }
}

