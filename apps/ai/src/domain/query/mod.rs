//! ユーザークエリ・assistant turn・query 入出力のドメインモデルと判断ロジック（純関数）

pub mod agent_loop_config;
pub mod command;
pub mod completion_heuristic;
pub mod query;
pub mod query_retry;

pub use agent_loop_config::{default_max_queries, DEFAULT_MAX_TURNS};
pub use command::AiCommand;
pub use completion_heuristic::{
    default_followup, looks_like_command_block, looks_like_question_or_need_user_input,
};
pub use query::Query;
pub use query_retry::QueryRetry;
