use common::domain::CatalogScope;
use std::path::PathBuf;

/// Task / Skill / Prompt などの「どこから来たか」を表す情報
///
/// - scope: project / user config / legacy 等のスコープ
/// - package_name: package 由来の場合の package 名（それ以外は None）
/// - path: 実体ファイルのパス（prompt.md, skill.toml 等）。hooks 等は "(hooks)" 等の論理名
/// - note: 補足（legacy / fallback / synthesized 等）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProvenance {
    pub scope: CatalogScope,
    pub package_name: Option<String>,
    pub path: PathBuf,
    pub note: Option<String>,
}

impl SourceProvenance {
    pub fn new(
        scope: CatalogScope,
        package_name: Option<String>,
        path: PathBuf,
        note: Option<String>,
    ) -> Self {
        Self {
            scope,
            package_name,
            path,
            note,
        }
    }
}

/// タスクの出所（dry-run / explain 表示用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOriginInfo {
    pub task_name: String,
    pub provenance: SourceProvenance,
}
