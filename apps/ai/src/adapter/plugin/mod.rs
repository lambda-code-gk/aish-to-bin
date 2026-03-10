//! プラグイン discovery・外部ツール登録の標準アダプタ
//!
//! - discovery: `libs/plugins`（`plugins::discovery`）を正本とし、ここでは直接 manifest
//!   を読まない。
//! - execution: `external_plugin_loader` / `external_plugin_stdio_client` / `external_tool_executor_impl`
//!   が、発見済みプラグインの起動と Tool 化のみを担当する。

pub(crate) mod external_plugin_loader;
pub(crate) mod external_plugin_manifest_loader;
pub(crate) mod external_plugin_stdio_client;
pub(crate) mod external_tool_executor_impl;

#[cfg(test)]
mod external_plugin_loader_tests;
#[cfg(test)]
mod external_plugin_stdio_client_tests;
