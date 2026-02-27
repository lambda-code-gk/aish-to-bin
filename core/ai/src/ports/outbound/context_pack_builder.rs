//! 履歴＋クエリから ContextPack を構築する Outbound ポート

use crate::domain::{ContextPack, Query};
use common::error::Error;
use common::llm::provider::Message as LlmMessage;

use super::QueryPlacement;

/// 履歴とクエリから ContextPack（メッセージ列 + 予算レポート）を構築する
pub trait ContextPackBuilder: Send + Sync {
    fn build(
        &self,
        history: &[LlmMessage],
        query: Option<&Query>,
        system_instruction: Option<&str>,
        query_placement: QueryPlacement,
    ) -> Result<ContextPack, Error>;
}
