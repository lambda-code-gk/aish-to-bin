use common::domain::CatalogScope;
use std::path::PathBuf;

/// Task / Skill / Prompt などの「どこから来たか」を表す情報
///
/// - scope: project / user config / legacy 等のスコープ
/// - package_name: package 由来の場合の package 名（それ以外は None）
/// - path: 実体ファイルのパス（prompt.md, skill.toml 等）
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProvenance {
    pub scope: CatalogScope,
    pub package_name: Option<String>,
    pub path: PathBuf,
}

impl SourceProvenance {
    #[allow(dead_code)]
    pub fn new(scope: CatalogScope, package_name: Option<String>, path: PathBuf) -> Self {
        Self {
            scope,
            package_name,
            path,
        }
    }
}

