//! 外部プラグインのツールを AISH の Tool として登録するプロキシ
//!
//! ツール名は本体にハードコードせず、プラグインの list_tools 結果をそのまま使う。
//! 外部プラグイン対応用（将来有効化予定）。
#![allow(dead_code)]

use crate::ports::outbound::ExternalToolExecutor;
use common::domain::event::{Event, RunId, SessionId};
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::sync::Arc;

/// 外部ツール 1 件分のプロキシ（name/description は 'static のため登録時に確定）
pub struct ExternalToolProxy {
    name: &'static str,
    description: &'static str,
    parameters_schema: Value,
    plugin_id: crate::domain::external_plugin::ExternalPluginId,
    executor: Arc<dyn ExternalToolExecutor>,
}

impl ExternalToolProxy {
    /// 登録用。name と description は呼び元で Box::leak して 'static にしたものを渡す。
    pub fn new(
        name: &'static str,
        description: &'static str,
        parameters_schema: Value,
        plugin_id: crate::domain::external_plugin::ExternalPluginId,
        executor: Arc<dyn ExternalToolExecutor>,
    ) -> Self {
        Self {
            name,
            description,
            parameters_schema,
            plugin_id,
            executor,
        }
    }
}

impl Tool for ExternalToolProxy {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(self.parameters_schema.clone())
    }

    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let start = std::time::Instant::now();
        let session_id = ctx
            .session_id
            .as_ref()
            .cloned()
            .unwrap_or_else(|| SessionId::new(""));
        let run_id = ctx
            .run_id
            .as_ref()
            .cloned()
            .unwrap_or_else(|| RunId::new(""));

        if let (Some(ref hub), _, _) = (&ctx.event_hub, &ctx.session_id, &ctx.run_id) {
            hub.emit(Event {
                v: 1,
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                kind: "external_tool.call_requested".to_string(),
                payload: serde_json::json!({
                    "plugin_id": self.plugin_id.0,
                    "tool_name": self.name,
                }),
            });
        }

        let result = self
            .executor
            .call_tool(&self.plugin_id, self.name, args.clone());

        let elapsed_ms = start.elapsed().as_millis() as u64;

        match &result {
            Ok(content) => {
                if let (Some(ref hub), _, _) = (&ctx.event_hub, &ctx.session_id, &ctx.run_id) {
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "external_tool.call_completed".to_string(),
                        payload: serde_json::json!({
                            "plugin_id": self.plugin_id.0,
                            "tool_name": self.name,
                            "elapsed_ms": elapsed_ms,
                        }),
                    });
                }
                Ok(content.clone())
            }
            Err(e) => {
                if let (Some(ref hub), _, _) = (&ctx.event_hub, &ctx.session_id, &ctx.run_id) {
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "external_tool.call_failed".to_string(),
                        payload: serde_json::json!({
                            "plugin_id": self.plugin_id.0,
                            "tool_name": self.name,
                            "error": e.to_string(),
                        }),
                    });
                }
                Err(ToolError::ExecutionFailed(e.to_string()))
            }
        }
    }
}
