use crate::discovery::{discover_plugins, DiscoveredPlugin, PluginToml};
use crate::stdio_jsonrpc::StdioJsonRpcClient;
use common::error::Error;
use common::ports::outbound::{
    McpCallContext, McpCallResult, McpHost, McpServerDescriptor, McpServerId, McpToolId,
    ToolDescriptor,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone, Deserialize)]
struct ListToolsItem {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    input_schema: Value,
}

fn canonical_tool_id(namespace: &str, tool_name: &str) -> McpToolId {
    if tool_name.contains('.') {
        McpToolId::new(tool_name.to_string())
    } else {
        McpToolId::new(format!("{}.{}", namespace, tool_name))
    }
}

#[derive(Debug, Clone)]
struct ToolBinding {
    server_id: McpServerId,
    tool_name: String,
}

fn pick_env_allowlisted(allow: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for k in allow {
        if let Ok(v) = std::env::var(k) {
            out.push((k.clone(), v));
        }
    }
    out
}

struct ServerRuntime {
    plugin: DiscoveredPlugin,
    client: Option<StdioJsonRpcClient>,
}

impl ServerRuntime {
    fn ensure_started(&mut self) -> Result<(), Error> {
        if self.client.is_some() {
            return Ok(());
        }
        let cfg = &self.plugin.config;
        let env = pick_env_allowlisted(&cfg.env_allowlist);
        let startup_timeout_ms = 10_000;
        let call_timeout_ms = cfg.timeout_ms.unwrap_or(30_000);
        let client = StdioJsonRpcClient::start(
            &cfg.command,
            &cfg.args,
            cfg.cwd.as_deref(),
            &env,
            startup_timeout_ms,
            call_timeout_ms,
        )?;
        self.client = Some(client);
        Ok(())
    }

    fn list_tools(&mut self) -> Result<Vec<(ToolDescriptor, String)>, Error> {
        self.ensure_started()?;
        let cfg = &self.plugin.config;
        let client = self.client.as_mut().expect("client started");
        let items = client.list_tools(cfg.timeout_ms)?;
        let mut out: Vec<(ToolDescriptor, String)> = Vec::new();
        for v in items {
            let item: ListToolsItem = serde_json::from_value(v)
                .map_err(|e| Error::json(format!("tool descriptor: {}", e)))?;
            let id = canonical_tool_id(&cfg.namespace, &item.name);
            let schema = if item.input_schema.is_object() {
                item.input_schema
            } else {
                serde_json::json!({ "type": "object", "properties": {} })
            };
            let desc = ToolDescriptor {
                id,
                display_name: if item.description.trim().is_empty() {
                    item.name.clone()
                } else {
                    item.description
                },
                schema,
                capabilities_hint: Vec::new(),
            };
            out.push((desc, item.name));
        }
        Ok(out)
    }

    fn call(
        &mut self,
        tool_id: &McpToolId,
        args: Value,
        ctx: McpCallContext,
    ) -> Result<McpCallResult, Error> {
        self.ensure_started()?;
        let cfg = &self.plugin.config;
        let client = self.client.as_mut().expect("client started");
        let tool_name = tool_id.0.clone();
        let (content, stderr_tail, elapsed_ms) =
            client.call_tool(&tool_name, args, ctx.timeout_ms.or(cfg.timeout_ms))?;
        Ok(McpCallResult {
            content,
            stderr_tail,
            elapsed_ms: Some(elapsed_ms),
        })
    }
}

/// stdio JSON-RPC プラグインを McpHost として提供する互換ブリッジ
///
/// - discovery: 信頼ディレクトリのみ（project `.aish/plugins/` + XDG `aish/plugins/` + legacy `plugins.d`）
/// - deny-by-default: enabled=false は list/call を拒否
/// - fail-closed: timeout などは Err を返し、timeout 時は子プロセスを kill
pub struct StdioJsonRpcMcpBridgeHost {
    plugins: RwLock<Vec<DiscoveredPlugin>>,
    runtimes: RwLock<HashMap<McpServerId, Arc<Mutex<ServerRuntime>>>>,
    tool_bindings: RwLock<HashMap<McpToolId, ToolBinding>>,
}

impl Default for StdioJsonRpcMcpBridgeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioJsonRpcMcpBridgeHost {
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
            runtimes: RwLock::new(HashMap::new()),
            tool_bindings: RwLock::new(HashMap::new()),
        }
    }

    fn refresh(&self) -> Result<Vec<DiscoveredPlugin>, Error> {
        let plugins = discover_plugins()?;
        let mut guard = self
            .plugins
            .write()
            .map_err(|_| Error::system("plugins lock poisoned".to_string()))?;
        *guard = plugins.clone();
        Ok(plugins)
    }

    fn plugin_by_id<'a>(
        plugins: &'a [DiscoveredPlugin],
        id: &McpServerId,
    ) -> Option<&'a DiscoveredPlugin> {
        plugins.iter().find(|p| p.config.id == id.0)
    }

    fn get_or_create_runtime(
        &self,
        plugin: DiscoveredPlugin,
    ) -> Result<Arc<Mutex<ServerRuntime>>, Error> {
        let id = McpServerId::new(plugin.config.id.clone());
        {
            if let Ok(map) = self.runtimes.read() {
                if let Some(rt) = map.get(&id) {
                    return Ok(Arc::clone(rt));
                }
            }
        }
        let mut map = self
            .runtimes
            .write()
            .map_err(|_| Error::system("runtimes lock poisoned".to_string()))?;
        Ok(Arc::clone(map.entry(id).or_insert_with(|| {
            Arc::new(Mutex::new(ServerRuntime {
                plugin,
                client: None,
            }))
        })))
    }

    fn binding_for_tool_id(&self, tool_id: &McpToolId) -> Option<ToolBinding> {
        self.tool_bindings
            .read()
            .ok()
            .and_then(|m| m.get(tool_id).cloned())
    }
}

impl McpHost for StdioJsonRpcMcpBridgeHost {
    fn discover(&self) -> Result<Vec<McpServerDescriptor>, Error> {
        let plugins = self.refresh()?;
        let mut out = Vec::new();
        for p in plugins {
            let cfg: &PluginToml = &p.config;
            out.push(McpServerDescriptor {
                id: McpServerId::new(cfg.id.clone()),
                namespace: cfg.namespace.clone(),
                display_name: cfg.display_name.clone().unwrap_or_else(|| cfg.id.clone()),
                enabled: cfg.enabled,
                source: Some(p.source.clone()),
            });
        }
        Ok(out)
    }

    fn list_tools(&self, server_id: &McpServerId) -> Result<Vec<ToolDescriptor>, Error> {
        let plugins = {
            let guard = self
                .plugins
                .read()
                .map_err(|_| Error::system("plugins lock poisoned".to_string()))?;
            if guard.is_empty() {
                drop(guard);
                self.refresh()?
            } else {
                guard.clone()
            }
        };
        let plugin = Self::plugin_by_id(&plugins, server_id)
            .ok_or_else(|| Error::invalid_argument(format!("plugin not found: {}", server_id)))?
            .clone();
        if !plugin.config.enabled {
            return Err(Error::invalid_argument(format!(
                "plugin disabled: {} (deny-by-default)",
                server_id
            )));
        }
        let rt = self.get_or_create_runtime(plugin)?;
        let mut guard = rt
            .lock()
            .map_err(|_| Error::system("runtime lock poisoned".to_string()))?;
        let tools_and_names = guard.list_tools()?;
        // cache tool -> (server, tool_name)
        // tool_name は stdio 側の list_tools.name を保持する（canonical 化による不整合を避ける）
        let mut map = self
            .tool_bindings
            .write()
            .map_err(|_| Error::system("tool map poisoned".to_string()))?;
        let mut tools = Vec::new();
        for (t, tool_name) in tools_and_names {
            map.insert(
                t.id.clone(),
                ToolBinding {
                    server_id: server_id.clone(),
                    tool_name,
                },
            );
            tools.push(t);
        }
        Ok(tools)
    }

    fn call(
        &self,
        tool_id: &McpToolId,
        args: Value,
        ctx: McpCallContext,
    ) -> Result<McpCallResult, Error> {
        if self
            .plugins
            .read()
            .map_err(|_| Error::system("plugins lock poisoned".to_string()))?
            .is_empty()
        {
            let _ = self.refresh();
        }
        let binding = self.binding_for_tool_id(tool_id);
        let server_id = binding
            .as_ref()
            .map(|b| b.server_id.clone())
            .or_else(|| {
                // fallback: namespace prefix で推測（cache 未作成でも call できるようにする）
                let ns = tool_id.0.split('.').next()?.to_string();
                let plugins = self.plugins.read().ok()?.clone();
                let p = plugins
                    .into_iter()
                    .find(|p| p.config.namespace == ns && p.config.enabled)?;
                Some(McpServerId::new(p.config.id))
            })
            .ok_or_else(|| Error::invalid_argument(format!("tool not registered: {}", tool_id)))?;

        let plugins = self
            .plugins
            .read()
            .map_err(|_| Error::system("plugins lock poisoned".to_string()))?
            .clone();
        let plugin = Self::plugin_by_id(&plugins, &server_id)
            .ok_or_else(|| Error::invalid_argument(format!("plugin not found: {}", server_id)))?
            .clone();
        if !plugin.config.enabled {
            return Err(Error::invalid_argument(format!(
                "plugin disabled: {} (deny-by-default)",
                server_id
            )));
        }
        let rt = self.get_or_create_runtime(plugin)?;
        let mut guard = rt
            .lock()
            .map_err(|_| Error::system("runtime lock poisoned".to_string()))?;
        // binding がある場合は stdio 側の本来の tool_name で呼ぶ
        let effective_tool_id = if let Some(b) = binding {
            McpToolId::new(b.tool_name)
        } else {
            tool_id.clone()
        };
        guard.call(&effective_tool_id, args, ctx)
    }
}
