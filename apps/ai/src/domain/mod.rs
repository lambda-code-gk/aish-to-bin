//! ai 固有のドメイン型（型と不変条件）

pub mod approval;
pub mod config;
pub mod context;
pub mod dry_run_info;
pub mod external_plugin;
pub mod history;
pub mod lifecycle;
pub mod policy;
pub mod query;
pub mod resolved;
pub mod sensitive_filter;
pub mod session;
pub mod task_name;
pub mod tool;

pub use approval::{Approval, ToolApproval};
pub use common::domain::EventEnvelope;
pub use config::*;
pub use context::*;
pub use dry_run_info::DryRunInfo;
pub use history::*;
pub use lifecycle::{LifecycleEvent, QueryOutcome};
pub use policy::*;
pub use query::*;
pub use resolved::*;
pub use sensitive_filter::*;
pub use task_name::TaskName;
pub use tool::*;
