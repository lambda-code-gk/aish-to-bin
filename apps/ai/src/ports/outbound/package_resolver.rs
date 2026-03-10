use crate::domain::ResolvedPackage;
use common::error::Error;

/// RuntimeCatalog + PackageSpecLoader を用いて package 一覧 / 解決を行うポート
pub trait PackageResolver: Send + Sync {
    /// 利用可能なすべての package を列挙する。
    ///
    /// - RuntimeCatalog.locations(CatalogKind::Packages) の順序に従う
    /// - 同名 package が複数存在する場合も、そのまま含める（resolve_package では先勝ち）
    fn list_packages(&self) -> Result<Vec<ResolvedPackage>, Error>;

    /// 名前から単一の package を解決する。
    ///
    /// - 同名衝突時は先勝ち（project → user config の順）
    #[allow(dead_code)]
    fn resolve_package(&self, name: &str) -> Result<Option<ResolvedPackage>, Error>;
}

