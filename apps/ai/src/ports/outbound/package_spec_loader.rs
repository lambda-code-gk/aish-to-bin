use crate::domain::PackageSpec;
use common::error::Error;
use std::path::Path;

/// packages/*/package.toml から PackageSpec を読み込むポート
pub trait PackageSpecLoader: Send + Sync {
    /// 単一の package を読み込む。
    ///
    /// - package_root/package.toml を探す
    /// - ファイルが存在しない場合や壊れている場合は Ok(None)
    fn load_package_spec(&self, package_root: &Path) -> Result<Option<PackageSpec>, Error>;

    /// packages_root 直下の全 PackageSpec を列挙する。
    ///
    /// - 直下の子ディレクトリに package.toml があるもののみ対象
    /// - 壊れている package.toml は warn してスキップする。
    fn list_package_specs(&self, packages_root: &Path) -> Result<Vec<PackageSpec>, Error>;
}
