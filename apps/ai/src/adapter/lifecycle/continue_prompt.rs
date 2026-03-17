//! 旧 lifecycle/continue_prompt 実装への互換レイヤ（新しい adapters/outbound/lifecycle/continue_prompt.rs への shim）

pub(crate) use crate::adapters::outbound::lifecycle::{CliContinuePrompt, NoContinuePrompt};
