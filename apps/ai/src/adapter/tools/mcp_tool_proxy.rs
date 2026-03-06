//! McpHost 経由で外部ツールを Tool として登録するプロキシ

use common::error::Error;
use common::ports::outbound::{McpCallContext, McpHost, McpToolId};
use common::tool::{Tool, ToolContext, ToolError};
use serde_json::Value;
use std::sync::Arc;

pub struct McpToolProxy {
    name: &'static str,
    description: &'static str,
    parameters_schema: Value,
    tool_id: McpToolId,
    host: Arc<dyn McpHost>,
}

impl McpToolProxy {
    pub fn new(
        name: &'static str,
        description: &'static str,
        parameters_schema: Value,
        tool_id: McpToolId,
        host: Arc<dyn McpHost>,
    ) -> Self {
        Self {
            name,
            description,
            parameters_schema,
            tool_id,
            host,
        }
    }
}

impl Tool for McpToolProxy {
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
        let mctx = McpCallContext {
            timeout_ms: None,
            session_id: ctx.session_id.as_ref().map(|s| s.to_string()),
            run_id: ctx.run_id.as_ref().map(|r| r.to_string()),
            non_interactive: false,
        };
        match self.host.call(&self.tool_id, args, mctx) {
            Ok(r) => Ok(r.content),
            Err(e) => {
                let msg = match e {
                    Error::InvalidArgument(m) => format!("InvalidArgument: {}", m),
                    other => other.to_string(),
                };
                Err(ToolError::ExecutionFailed(msg))
            }
        }
    }
}
