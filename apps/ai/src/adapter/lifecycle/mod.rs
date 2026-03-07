//! ライフサイクルフックのアダプタ（コンポジット + 個別ハンドラ）

mod composite;
mod continue_prompt;
mod self_improve;

pub(crate) use composite::{CompositeLifecycleHooks, LifecycleHandler};
pub(crate) use continue_prompt::{CliContinuePrompt, NoContinuePrompt};
pub(crate) use self_improve::SelfImproveHandler;
