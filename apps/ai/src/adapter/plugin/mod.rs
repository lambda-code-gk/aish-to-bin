//! プラグイン discovery・外部ツール登録の標準アダプタ

pub(crate) mod external_plugin_loader;
pub(crate) mod external_plugin_manifest_loader;
pub(crate) mod external_plugin_stdio_client;
pub(crate) mod external_tool_executor_impl;

#[cfg(test)]
mod external_plugin_loader_tests;
#[cfg(test)]
mod external_plugin_stdio_client_tests;
