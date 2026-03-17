//! ライフサイクルフックのアダプタ（コンポジット + 個別ハンドラ）の旧配置 shim
//!
//  実体は adapters::outbound::lifecycle 配下に移動済み。
//  旧パス互換のため mod 構造と re-export だけ維持する。

mod composite;
mod continue_prompt;
mod self_improve;

pub(crate) use crate::adapters::outbound::lifecycle::LifecycleHandler;
pub(crate) use crate::adapters::outbound::lifecycle::{
    CliContinuePrompt, CompositeLifecycleHooks, NoContinuePrompt, SelfImproveHandler,
};
