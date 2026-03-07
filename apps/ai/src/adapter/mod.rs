//! 標準アダプタ（config / context / lifecycle / llm / plugin / policy / session / tools）

pub(crate) mod approval;
pub(crate) mod config;
pub(crate) mod context;
pub(crate) mod dry_run_report_sink;
pub(crate) mod lifecycle;
pub(crate) mod llm;
pub(crate) mod plugin;
pub(crate) mod policy;
pub(crate) mod session;
pub(crate) mod sigint_checker;
pub(crate) mod sinks;
pub(crate) mod task;
pub(crate) mod tools;

// Re-exports: 旧フラット構造に合わせて crate 内参照を維持
pub(crate) use approval::{CliToolApproval, NonInteractiveToolApproval};
#[allow(unused_imports)] // テスト・wiring で参照
pub(crate) use config::{
    CliPolicyOverrides, ConfigurableToolProfileProvider, StaticToolProfileProvider,
    StdCommandAllowRulesLoader, StdConfigExplainProvider, StdConfigProvider, StdProfileLister,
    StdResolveModeConfig, StdResolveProfileAndModel, StdResolveSystemPromptFromHooks,
};
#[allow(unused_imports)] // テストで StdContextMessageBuilder, StdContextPackBuilder 等を参照
pub(crate) use context::{
    ChangedFilesSnippetSelector, GrepHitsSelector, MemorySelector, PassThroughReducer,
    StdContextArtifactStore, StdContextMessageBuilder, StdContextPackBuilder,
    StdContextPackBuilderWithAddons, StdResolveMemoryDir, TailWindowReducer,
};
pub(crate) use dry_run_report_sink::StdoutDryRunReportSink;
pub(crate) use lifecycle::{
    CliContinuePrompt, CompositeLifecycleHooks, NoContinuePrompt, SelfImproveHandler,
};
pub(crate) use llm::{StdLlmCompletion, StdLlmEventStreamFactory};
pub(crate) use policy::{
    EgressBudgetHardCapRule, EgressSensitiveRule, LeakscanPrepareSession, LeakscanTextFilter,
    ShellAllowlistRule, StdPolicyEngine, StdPolicyExplainProvider, ToolModeRule,
};
pub(crate) use session::{
    DaemonEventAppender, DeterministicCompactionStrategy, FallbackEventAppender,
    FileAgentStateStorage, ManifestReviewedSessionStorage, ManifestTailCompactionViewStrategy,
    PartSessionStorage, ReviewedTailViewStrategy, StdSessionDerivedBuilder,
};
pub(crate) use sigint_checker::{NoopInterruptChecker, SigintChecker};
pub(crate) use sinks::StdEventSinkFactory;
pub(crate) use task::StdTaskRunner;
pub(crate) use tools::{
    GetMemoryContentTool, GrepTool, HistoryGetTool, HistorySearchTool, McpToolProxy,
    QueueShellSuggestionTool, ReadFileTool, ReplaceFileTool, SaveMemoryTool, SearchMemoryTool,
    ShellTool, WriteFileTool,
};
