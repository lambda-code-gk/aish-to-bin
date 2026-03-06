//! 外部ツール実行 Outbound ポート
//!
//! プラグインの call_tool を中継する。usecase は知らず、Tool 実装（ExternalToolProxy）が使用する。
//! 外部プラグイン対応用（将来有効化予定）。
#![allow(dead_code)]

use crate::domain::external_plugin::{ExternalPluginError, ExternalPluginId};
use serde_json::Value;

/// 外部ツール呼び出しを実行する（プラグインごとの call_tool 中継）
pub trait ExternalToolExecutor: Send + Sync {
    fn call_tool(
        &self,
        plugin_id: &ExternalPluginId,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, ExternalPluginError>;
}
