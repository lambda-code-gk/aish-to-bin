//! 責務: daemon request を usecase/port に委譲して処理する。

use std::sync::Arc;

use common::error::Error;
use common::ports::outbound::McpHost;

use crate::daemon_bridge::{self, DaemonRequestHandler};
use crate::usecase::{HistoryUseCase, MemoryUseCase};

pub(crate) struct WiredDaemonRequestHandler {
    memory_use_case: MemoryUseCase,
    history_use_case: HistoryUseCase,
    mcp_host: Arc<dyn McpHost>,
}

impl WiredDaemonRequestHandler {
    pub(crate) fn new(
        memory_use_case: MemoryUseCase,
        history_use_case: HistoryUseCase,
        mcp_host: Arc<dyn McpHost>,
    ) -> Self {
        Self {
            memory_use_case,
            history_use_case,
            mcp_host,
        }
    }
}

impl DaemonRequestHandler for WiredDaemonRequestHandler {
    fn run_read(&self, request: daemon_api::AishReadRequest) -> Result<serde_json::Value, Error> {
        match request {
            daemon_api::AishReadRequest::MemoryList { .. } => {
                serde_json::to_value(self.memory_use_case.list()?)
                    .map_err(|e| Error::json(e.to_string()))
            }
            daemon_api::AishReadRequest::MemoryGet { ids, .. } => {
                serde_json::to_value(self.memory_use_case.get(&ids)?)
                    .map_err(|e| Error::json(e.to_string()))
            }
            daemon_api::AishReadRequest::HistoryList {
                context,
                session_explicitly_specified,
                all,
                user_only,
                assistant_only,
            } => {
                let path_input = daemon_bridge::path_input_from_context(&context);
                serde_json::to_value(self.history_use_case.list(
                    &path_input,
                    session_explicitly_specified,
                    all,
                    user_only,
                    assistant_only,
                )?)
                .map_err(|e| Error::json(e.to_string()))
            }
            daemon_api::AishReadRequest::HistoryGet {
                context,
                session_explicitly_specified,
                ids,
            } => {
                let path_input = daemon_bridge::path_input_from_context(&context);
                serde_json::to_value(self.history_use_case.get(
                    &path_input,
                    session_explicitly_specified,
                    &ids,
                )?)
                .map_err(|e| Error::json(e.to_string()))
            }
            daemon_api::AishReadRequest::PluginsList { .. } => {
                let mut list = self.mcp_host.discover()?;
                list.sort_by(|a, b| a.id.0.cmp(&b.id.0));
                serde_json::to_value(list).map_err(|e| Error::json(e.to_string()))
            }
            daemon_api::AishReadRequest::ToolsList { .. } => {
                let mut tool_ids: Vec<String> = Vec::new();
                let mut warnings: Vec<String> = Vec::new();
                let servers = self.mcp_host.discover()?;
                for server in servers.into_iter().filter(|s| s.enabled) {
                    let tools = match self.mcp_host.list_tools(&server.id) {
                        Ok(tools) => tools,
                        Err(e) => {
                            warnings.push(format!(
                                "warning: failed to list tools for plugin '{}': {}",
                                server.id.0, e
                            ));
                            continue;
                        }
                    };
                    for tool in tools {
                        tool_ids.push(tool.id.0);
                    }
                }
                tool_ids.sort();
                tool_ids.dedup();
                serde_json::to_value(daemon_api::AishToolsListResult { tool_ids, warnings })
                    .map_err(|e| Error::json(e.to_string()))
            }
        }
    }

    fn run_write(&self, request: daemon_api::AishWriteRequest) -> Result<serde_json::Value, Error> {
        match request {
            daemon_api::AishWriteRequest::MemoryRemove { ids, .. } => {
                self.memory_use_case.remove(&ids)?;
                Ok(serde_json::Value::Null)
            }
        }
    }
}
