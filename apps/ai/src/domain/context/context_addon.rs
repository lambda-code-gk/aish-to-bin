//! コンテキストに追加する補助情報（ファイルスニペット等）

use super::{ContextAttachment, ContextSource};
use common::msg::Msg;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextAddon {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub priority: u32,
    pub msg: Msg,
    pub attachment: Option<ContextAttachment>,
    pub source: Option<ContextSource>,
}
