//! ライフサイクルフックのアダプタ（コンポジット + 個別ハンドラ）

pub(crate) mod composite;
pub(crate) mod continue_prompt;
pub(crate) mod self_improve;

pub(crate) use composite::{CompositeLifecycleHooks, LifecycleHandler};
pub(crate) use continue_prompt::{CliContinuePrompt, NoContinuePrompt};
pub(crate) use self_improve::SelfImproveHandler;

