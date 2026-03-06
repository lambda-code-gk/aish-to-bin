//! ExternalToolExecutor の実装。プラグイン ID ごとに Stdio クライアントを保持し、call_tool を中継する。
//! 外部プラグイン対応用（将来有効化予定）。
#![allow(dead_code)]

use crate::adapter::external_plugin_stdio_client::ExternalPluginStdioClient;
use crate::domain::external_plugin::{ExternalPluginError, ExternalPluginId};
use crate::ports::outbound::ExternalToolExecutor;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct ExternalToolExecutorImpl {
    clients: RwLock<HashMap<ExternalPluginId, Arc<ExternalPluginStdioClient>>>,
}

impl ExternalToolExecutorImpl {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, plugin_id: ExternalPluginId, client: Arc<ExternalPluginStdioClient>) {
        self.clients.write().unwrap().insert(plugin_id, client);
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.clients.read().unwrap().is_empty()
    }
}

impl ExternalToolExecutor for ExternalToolExecutorImpl {
    fn call_tool(
        &self,
        plugin_id: &ExternalPluginId,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ExternalPluginError> {
        let guard = self.clients.read().map_err(|_| {
            ExternalPluginError::ToolCallFailed("executor lock poisoned".to_string())
        })?;
        let client = guard.get(plugin_id).ok_or_else(|| {
            ExternalPluginError::ToolCallFailed(format!("plugin not found: {}", plugin_id))
        })?;
        client.call_tool(tool_name, args)
    }
}
