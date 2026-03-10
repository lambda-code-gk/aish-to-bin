use crate::domain::{MemoryKind, MemoryScope};

/// 既存の MemoryEntry を論理的に正規化した構造化メモリエントリ
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StructuredMemoryEntry {
    pub id: String,
    pub kind: MemoryKind,
    pub topics: Vec<String>,
    pub title: Option<String>,
    pub summary: String,
    pub body: Option<String>,
    pub confidence: Option<f32>,
    pub updated_at: Option<String>,
    pub source_scope: MemoryScope,
}
