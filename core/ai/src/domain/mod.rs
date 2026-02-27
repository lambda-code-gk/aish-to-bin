//! ai 固有のドメイン型（型と不変条件）

pub mod approval;
pub mod budget_report;
pub mod command;
pub mod compaction;
pub mod context_addon;
pub mod context_pack;
pub mod dry_run_info;
pub mod external_plugin;
pub mod mode_config;
pub mod context_budget;
pub mod history;
pub mod history_reducer;
pub mod lifecycle;
pub mod manifest;
pub mod memory_entry;
pub mod query;
pub mod task_name;
pub use approval::{Approval, ToolApproval};
pub use budget_report::*;
pub use command::AiCommand;
pub use context_addon::*;
pub use context_pack::*;
pub use dry_run_info::DryRunInfo;
pub use mode_config::ModeConfig;
pub use compaction::CompactionRecord;
pub use context_budget::ContextBudget;
pub use history::History;
pub use history_reducer::HistoryReducer;
pub use lifecycle::{LifecycleEvent, QueryOutcome};
pub use manifest::{
    hash64, parse_lines, ManifestDecision, ManifestRecordV1, ManifestRole, MessageRecordV1,
};
pub use memory_entry::{MemoryEntry, MemoryMeta};
pub use query::Query;
pub use task_name::TaskName;
