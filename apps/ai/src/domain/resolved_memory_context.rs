use crate::domain::StructuredMemoryEntry;

/// topic / kind ベースで解決されたメモリコンテキスト
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ResolvedMemoryContext {
    pub entries: Vec<StructuredMemoryEntry>,
    /// プロンプトに差し込む短い要約テキスト
    pub rendered_summary: String,
}

