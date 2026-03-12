//! 責務: プロンプト解決の判断ロジック（タスクレイアウト判定・候補の組み立て順序）のみ。I/O を知らない。

pub mod task_layout;
pub mod candidate;
pub mod assembly;

pub use task_layout::{detect_task_kind, task_prompt_path, TaskKind};
pub use candidate::PromptCandidate;
pub use assembly::PromptAssemblyDecision;
