use crate::domain::MemoryKind;

/// 構造化メモリの検索クエリ
#[derive(Debug, Clone)]
pub struct MemoryQuery {
    /// 関連トピック（小文字・trim 済みで扱う想定）
    pub topics: Vec<String>,
    /// 対象とする kind。空のときは全種対象。
    pub kinds: Vec<MemoryKind>,
    /// 返す件数の上限
    pub limit: usize,
}

impl MemoryQuery {
    pub fn new(topics: Vec<String>, kinds: Vec<MemoryKind>, limit: usize) -> Self {
        Self {
            topics,
            kinds,
            limit,
        }
    }
}

