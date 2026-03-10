/// メモリのスコープ（プロジェクト or グローバル）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScope {
    Project,
    Global,
}
