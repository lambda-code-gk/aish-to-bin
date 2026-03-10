//! Plugin discovery and stdio JSON-RPC bridge host.
//!
//! - Discovery (canonical): see `discovery` module. It defines how `plugin.toml`
//!   and legacy YAML manifests are loaded, how locations from `RuntimeCatalog`
//!   are interpreted, and how duplicate ids / enabled flags are handled.
//! - Execution: this crate does **not** start plugin processes itself; that is
//!   the responsibility of application crates such as `apps/ai` and `apps/aish`.

pub mod discovery;
mod stdio_bridge_host;
mod stdio_jsonrpc;

pub use discovery::{
    discover_plugins, discover_plugins_with_catalog, DiscoveredPlugin, PluginToml,
};
pub use stdio_bridge_host::StdioJsonRpcMcpBridgeHost;
