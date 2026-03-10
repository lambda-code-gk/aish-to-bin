use crate::domain::PackageSpec;
use common::domain::CatalogScope;

/// RuntimeCatalog + PackageSpecLoader によって解決された package
///
/// - spec: package.toml 由来のメタデータ
/// - scope: プロジェクト / ユーザー設定などの出所（RuntimeCatalog に準拠）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackage {
    pub spec: PackageSpec,
    pub scope: CatalogScope,
}
