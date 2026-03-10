//! プラグインを発見・起動し、list_tools で取得したツールを Tool として返す。
//!
//! - discovery の正本は `libs/plugins`（`plugins::discovery`）にある。
//! - このモジュールは「発見済みプラグインを起動し、list_tools 結果を Tool に変換する」
//!   実行責務のみに絞る。
//! - fail-closed: 起動失敗・list_tools 失敗したプラグインはスキップし、他は継続。
//! - plugin_id の重複は discovery 層で先勝ちとなる（ここでは Tool 名の衝突のみ検出）。
#![allow(dead_code)]

use super::external_plugin_stdio_client::ExternalPluginStdioClient;
use super::external_tool_executor_impl::ExternalToolExecutorImpl;
use crate::adapter::tools::ExternalToolProxy;
use crate::domain::external_plugin::{ExternalPluginId, PluginTimeouts, StdioTransport};
use common::domain::event::{Event, RunId, SessionId};
use common::event_hub::EventHubHandle;
use plugins::discovery::{discover_plugins, DiscoveredPlugin};
use std::collections::HashSet;
use std::sync::Arc;

/// プラグインを発見・起動し、外部ツールの Tool 一覧を返す。Executor は各 Proxy が共有する。
pub fn load_external_plugins(
    event_hub: Option<EventHubHandle>,
) -> Vec<Arc<dyn common::tool::Tool>> {
    let session_id = SessionId::new("bootstrap");
    let run_id = RunId::new("plugins");

    let discovered = match discover_plugins() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let executor = Arc::new(ExternalToolExecutorImpl::new());
    let mut tools: Vec<Arc<dyn common::tool::Tool>> = Vec::new();
    let mut registered_names: HashSet<String> = HashSet::new();
    for DiscoveredPlugin { config, .. } in discovered {
        // discovery 層では enabled フラグをそのまま返す。実行側では enabled=false の
        // プラグインは起動しない（fail-closed）。
        if !config.enabled {
            continue;
        }

        let plugin_id = ExternalPluginId::new(config.id.clone());
        let transport = StdioTransport {
            command: config.command.clone(),
            args: config.args.clone(),
        };
        // env_allowlist された環境変数だけを子プロセスに渡す。
        let env_map: std::collections::HashMap<String, String> = config
            .env_allowlist
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.clone(), v)))
            .collect();
        let timeouts = PluginTimeouts {
            startup_ms: Some(10_000),
            call_ms: config.timeout_ms.or(Some(30_000)),
        };

        if let Some(ref hub) = event_hub {
            hub.emit(Event {
                v: 1,
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                kind: "external_plugin.start_requested".to_string(),
                payload: serde_json::json!({
                    "plugin_id": plugin_id.0,
                    "command": transport.command,
                }),
            });
        }

        let client = match ExternalPluginStdioClient::start(&transport, &env_map, &timeouts) {
            Ok(c) => c,
            Err(e) => {
                if let Some(ref hub) = event_hub {
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "external_plugin.start_failed".to_string(),
                        payload: serde_json::json!({
                            "plugin_id": plugin_id.0,
                            "error": e.to_string(),
                        }),
                    });
                }
                continue;
            }
        };

        if let Some(ref hub) = event_hub {
            hub.emit(Event {
                v: 1,
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                kind: "external_plugin.started".to_string(),
                payload: serde_json::json!({
                    "plugin_id": plugin_id.0,
                }),
            });
        }

        let client = Arc::new(client);
        let descriptors = match client.list_tools() {
            Ok(d) => d,
            Err(e) => {
                if let Some(ref hub) = event_hub {
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "external_plugin.list_tools_failed".to_string(),
                        payload: serde_json::json!({
                            "plugin_id": plugin_id.0,
                            "error": e.to_string(),
                        }),
                    });
                }
                continue;
            }
        };

        executor.register(plugin_id.clone(), Arc::clone(&client));

        if let Some(ref hub) = event_hub {
            let tool_names: Vec<&str> = descriptors.iter().map(|d| d.name.as_str()).collect();
            hub.emit(Event {
                v: 1,
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                kind: "external_plugin.tools_listed".to_string(),
                payload: serde_json::json!({
                    "plugin_id": plugin_id.0,
                    "tool_count": descriptors.len(),
                    "tool_names": tool_names,
                }),
            });
        }

        for desc in descriptors {
            if !registered_names.insert(desc.name.clone()) {
                if let Some(ref hub) = event_hub {
                    hub.emit(Event {
                        v: 1,
                        session_id: session_id.clone(),
                        run_id: run_id.clone(),
                        kind: "external_plugin.tool_name_conflict".to_string(),
                        payload: serde_json::json!({
                            "plugin_id": plugin_id.0,
                            "tool_name": desc.name,
                        }),
                    });
                }
                continue;
            }
            let name_static: &'static str = Box::leak(desc.name.clone().into_boxed_str());
            let desc_static: &'static str = Box::leak(desc.description.clone().into_boxed_str());
            let schema = desc
                .input_schema
                .as_object()
                .map(|o| serde_json::Value::Object(o.clone()))
                .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} }));

            let proxy = ExternalToolProxy::new(
                name_static,
                desc_static,
                schema,
                plugin_id.clone(),
                Arc::clone(&executor) as Arc<dyn crate::ports::outbound::ExternalToolExecutor>,
            );
            tools.push(Arc::new(proxy));
        }
    }

    tools
}
