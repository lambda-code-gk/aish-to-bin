use std::path::PathBuf;

/// カタログ対象の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    /// task.d によるタスク
    Tasks,
    /// hooks/system_prompt によるシステムプロンプトフック
    SystemPromptHooks,
    /// .aish/plugins / plugins.d による外部プラグイン
    Plugins,
    /// skills ディレクトリによる skill 定義
    Skills,
    /// packages ディレクトリによる package 定義
    ///
    /// - {project_root}/.aish/packages
    /// - {config_dir}/packages
    ///
    /// の順で探索する（同名 package は先勝ち）。
    Packages,
}

/// カタログ項目のスコープ（出所）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogScope {
    /// プロジェクトローカル（{project_root}/.aish/...）
    Project,
    /// ユーザー設定（config dir 配下: resolve_dirs().config_dir）
    UserConfig,
    /// 互換レガシーパス（~/.aish や plugins.d 等）
    LegacyUser,
    /// 将来拡張用のシステムスコープ
    System,
}

/// 実行時に参照される資源の所在
#[derive(Debug, Clone)]
pub struct CatalogLocation {
    pub kind: CatalogKind,
    pub scope: CatalogScope,
    pub path: PathBuf,
}
