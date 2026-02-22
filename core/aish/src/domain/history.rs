//! reviewed 履歴の一覧・取得用ドメイン型

/// history ls の1行分: id, 日時, 先頭1行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryListEntry {
    pub id: String,
    pub datetime: String,
    pub first_line: String,
}

/// history get の1件分: id と本文
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryGetEntry {
    pub id: String,
    pub content: String,
}
