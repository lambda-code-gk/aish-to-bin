//! 文脈選択・context budget・context pack のドメインモデルと判断ロジック（純関数）

pub mod addon_screening;
pub mod assembly;
pub mod budget_report;
pub mod budget_report_builder;
pub mod context_addon;
pub mod context_budget;
pub mod context_pack;
pub mod history_pack;
pub mod memory_dedup;
pub mod memory_render;
pub mod selection_policy;

pub use addon_screening::{apply_sensitive_outcome_to_addon, SensitiveCheckResult};
pub use assembly::{assemble_context_with_addons, ContextAssemblyPlan};
pub use budget_report::*;
pub use budget_report_builder::build_budget_report;
pub use context_addon::*;
pub use context_budget::ContextBudget;
pub use context_pack::*;
pub use history_pack::{decide_history_pack, HistoryPackDecision};
pub use memory_dedup::{merge_memory_entries_project_first, normalize_topics};
pub use memory_render::render_memory_context;
pub use selection_policy::{addon_insertion_index, select_addons_within_budget};
