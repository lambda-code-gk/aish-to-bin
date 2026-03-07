//! 信頼ディレクトリからプラグインを発見・起動し、list_tools で取得したツールを Tool として返す。
//!
//! fail-closed: 起動失敗・list_tools 失敗したプラグインはスキップし、他は継続。
//! plugin_id は一意必須。重複時は先勝ち（最初に処理した manifest のみ有効化、後続は skipped_id_conflict でスキップ）。
//! 外部プラグイン対応は将来有効化予定のため、現状は dead_code を許容。
#![allow(dead_code)]

use super::external_plugin_manifest_loader::discover_manifests;
use super::external_plugin_stdio_client::ExternalPluginStdioClient;
use super::external_tool_executor_impl::ExternalToolExecutorImpl;
use crate::adapter::tools::ExternalToolProxy;
use crate::domain::external_plugin::{ExternalPluginId, PluginTransport};
use common::domain::event::{Event, RunId, SessionId};
use common::event_hub::EventHubHandle;
use common::ports::outbound::{EnvResolver, FileSystem};
use std::collections::HashSet;
use std::sync::Arc;

/// プラグインを発見・起動し、外部ツールの Tool 一覧を返す。Executor は各 Proxy が共有する。
pub fn load_external_plugins(
    fs: Arc<dyn FileSystem>,
    env: Arc<dyn EnvResolver>,
    event_hub: Option<EventHubHandle>,
) -> Vec<Arc<dyn common::tool::Tool>> {
    let session_id = SessionId::new("bootstrap");
    let run_id = RunId::new("plugins");

    let entries = match discover_manifests(fs.clone(), env.clone(), event_hub.as_ref()) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let executor = Arc::new(ExternalToolExecutorImpl::new());
    let mut tools: Vec<Arc<dyn common::tool::Tool>> = Vec::new();
    let mut registered_names: HashSet<String> = HashSet::new();
    // plugin_id 重複時は先勝ち。後続はスキップして誤配送を防ぐ。
    let mut seen_plugin_ids: HashSet<String> = HashSet::new();

    for entry in entries {
        let plugin_id = ExternalPluginId::new(entry.manifest.id.clone());
        if !seen_plugin_ids.insert(entry.manifest.id.clone()) {
            if let Some(ref hub) = event_hub {
                hub.emit(Event {
                    v: 1,
                    session_id: session_id.clone(),
                    run_id: run_id.clone(),
                    kind: "external_plugin.skipped_id_conflict".to_string(),
                    payload: serde_json::json!({
                        "plugin_id": plugin_id.0,
                        "manifest_path": entry.manifest_path.to_string_lossy(),
                    }),
                });
            }
            continue;
        }
        let (transport, env_map) = match &entry.manifest.transport {
            PluginTransport::Stdio(t) => (t, &entry.manifest.env),
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

        let client =
            match ExternalPluginStdioClient::start(transport, env_map, &entry.manifest.timeouts) {
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
