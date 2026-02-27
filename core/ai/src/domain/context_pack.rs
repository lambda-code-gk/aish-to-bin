//! LLM 送信前の文脈パック（メッセージ列 + 添付 + 予算レポート）

use super::BudgetReport;
use common::msg::Msg;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub v: u32,
    pub messages: Vec<Msg>,
    pub attachments: Vec<ContextAttachment>,
    pub budget_report: BudgetReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextAttachment {
    pub kind: String,
    pub title: String,
    pub content_type: String,
    pub content: Option<String>,
    pub artifact_rel_path: Option<String>,
    pub bytes: u64,
    pub hash64: String,
    pub source: Option<ContextSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSource {
    pub kind: String,
    pub ref_id: String,
}
