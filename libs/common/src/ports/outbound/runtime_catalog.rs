use crate::domain::{CatalogKind, CatalogLocation};
use crate::error::Error;
use std::path::PathBuf;

/// 実行時に参照するタスク / hooks / plugins / project root を一覧するカタログ
///
/// - Phase A ではパスと出所のみを返す
/// - 呼び出し側は探索順序を再実装せず、このカタログの結果に従う
pub trait RuntimeCatalog: Send + Sync {
    /// 指定 kind の候補パスを、優先順位順に返す。
    ///
    /// 戻り値には存在確認済みの候補のみ含める。
    fn locations(&self, kind: CatalogKind) -> Result<Vec<CatalogLocation>, Error>;

    /// プロジェクトルート（親方向に遡って `.aish` ディレクトリがある場所）を返す。
    ///
    /// - 見つからない場合は Ok(None)
    /// - current_dir 取得失敗時も Ok(None)（fail-closed しない）
    fn project_root(&self) -> Result<Option<PathBuf>, Error>;
}
