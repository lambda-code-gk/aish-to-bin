//! コンテキストに追加する補助情報を選択する Outbound ポート

use crate::domain::{ContextAddon, Query};
use common::domain::SessionDir;
use common::error::Error;
use common::llm::provider::Message as LlmMessage;
use std::path::Path;

/// selector に渡す入力
pub struct ContextAddonInput<'a> {
    pub history: &'a [LlmMessage],
    pub query: Option<&'a Query>,
    pub project_root: &'a Path,
    pub session_dir: Option<&'a SessionDir>,
}

/// 追加文脈を選択する
pub trait ContextAddonSelector: Send + Sync {
    fn name(&self) -> &str;
    fn select(&self, input: &ContextAddonInput) -> Result<Vec<ContextAddon>, Error>;
}
