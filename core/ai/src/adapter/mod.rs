pub(crate) mod agent_state_storage;
pub(crate) mod approval;
pub(crate) mod compactor_deterministic;
pub(crate) mod config;
pub(crate) mod config_explain_provider;
pub(crate) mod config_loader;
pub(crate) mod config_toml;
pub(crate) mod context_addon_selectors;
pub(crate) mod context_addon_selectors_grep;
pub(crate) mod context_addon_selectors_memory;
pub(crate) mod context_artifact_store;
pub(crate) mod daemon_event_appender;
pub(crate) mod fallback_event_appender;
pub(crate) mod context_message_builder;
pub(crate) mod context_pack_builder;
pub(crate) mod continue_prompt;
pub(crate) mod dry_run_report_sink;
pub(crate) mod external_plugin_loader;
pub(crate) mod external_plugin_manifest_loader;
pub(crate) mod external_plugin_stdio_client;
pub(crate) mod external_tool_executor_impl;
pub(crate) mod leakscan_prepare_session;
pub(crate) mod leakscan_text_filter;
pub(crate) mod lifecycle;
pub(crate) mod llm_completion;
pub(crate) mod llm_event_stream_factory;
pub(crate) mod manifest_reviewed_session_storage;
pub(crate) mod memory_storage;
pub(crate) mod part_session_storage;
pub(crate) mod policy_engine;
pub(crate) mod policy_explain_provider;
pub(crate) mod policy_rules_egress;
pub(crate) mod policy_rules_tool;
pub(crate) mod profile_lister;
pub(crate) mod reducer;
pub(crate) mod resolve_memory_dir;
pub(crate) mod resolve_mode_config;
pub(crate) mod resolve_profile_and_model;
pub(crate) mod resolve_system_prompt_from_hooks;
pub(crate) mod reviewed_session_storage;
pub(crate) mod session_derived_builder;
pub(crate) mod session_manifest;
pub(crate) mod sigint_checker;
pub(crate) mod sinks;
pub(crate) mod task;
pub(crate) mod tool_profile_provider;
pub(crate) mod tools;

#[cfg(test)]
mod external_plugin_loader_tests;
#[cfg(test)]
mod external_plugin_stdio_client_tests;
#[cfg(test)]
pub(crate) mod stub_llm;
pub(crate) use agent_state_storage::FileAgentStateStorage;
pub(crate) use approval::{CliToolApproval, NonInteractiveToolApproval};
pub(crate) use compactor_deterministic::DeterministicCompactionStrategy;
pub(crate) use config::StdCommandAllowRulesLoader;
pub(crate) use config_explain_provider::StdConfigExplainProvider;
pub(crate) use config_loader::{CliPolicyOverrides, StdConfigProvider};
pub(crate) use context_addon_selectors::ChangedFilesSnippetSelector;
pub(crate) use context_addon_selectors_grep::GrepHitsSelector;
pub(crate) use context_addon_selectors_memory::MemorySelector;
pub(crate) use context_artifact_store::StdContextArtifactStore;
pub(crate) use daemon_event_appender::DaemonEventAppender;
pub(crate) use fallback_event_appender::FallbackEventAppender;
// 以下はテストで参照（context_message_builder_tests, context_pack_builder_tests）。本ビルドでは未使用のため allow。
#[allow(unused_imports)]
pub(crate) use context_message_builder::StdContextMessageBuilder;
#[allow(unused_imports)]
pub(crate) use context_pack_builder::{StdContextPackBuilder, StdContextPackBuilderWithAddons};
pub(crate) use continue_prompt::{CliContinuePrompt, NoContinuePrompt};
pub(crate) use dry_run_report_sink::StdoutDryRunReportSink;
pub(crate) use leakscan_prepare_session::LeakscanPrepareSession;
pub(crate) use leakscan_text_filter::LeakscanTextFilter;
pub(crate) use lifecycle::{CompositeLifecycleHooks, SelfImproveHandler};
pub(crate) use llm_completion::StdLlmCompletion;
pub(crate) use llm_event_stream_factory::StdLlmEventStreamFactory;
pub(crate) use manifest_reviewed_session_storage::{
    ManifestReviewedSessionStorage, ManifestTailCompactionViewStrategy, ReviewedTailViewStrategy,
};
pub(crate) use part_session_storage::PartSessionStorage;
pub(crate) use policy_engine::StdPolicyEngine;
pub(crate) use policy_rules_egress::{EgressBudgetHardCapRule, EgressSensitiveRule};
pub(crate) use policy_rules_tool::{ShellAllowlistRule, ToolModeRule};
pub(crate) use profile_lister::StdProfileLister;
pub(crate) use reducer::{PassThroughReducer, TailWindowReducer};
pub(crate) use resolve_memory_dir::StdResolveMemoryDir;
pub(crate) use resolve_mode_config::StdResolveModeConfig;
pub(crate) use resolve_profile_and_model::StdResolveProfileAndModel;
pub(crate) use resolve_system_prompt_from_hooks::StdResolveSystemPromptFromHooks;
pub(crate) use session_derived_builder::StdSessionDerivedBuilder;
pub(crate) use sigint_checker::{NoopInterruptChecker, SigintChecker};
pub(crate) use sinks::StdEventSinkFactory;
pub(crate) use task::StdTaskRunner;
pub(crate) use tool_profile_provider::StaticToolProfileProvider;
pub(crate) use tools::{
    GetMemoryContentTool, GrepTool, HistoryGetTool, HistorySearchTool, QueueShellSuggestionTool,
    McpToolProxy, ReadFileTool, ReplaceFileTool, SaveMemoryTool, SearchMemoryTool, ShellTool,
    WriteFileTool,
};
