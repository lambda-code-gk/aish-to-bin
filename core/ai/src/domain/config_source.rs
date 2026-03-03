use serde::{Deserialize, Serialize};

/// 設定値の出典種別（どのレイヤから来たか）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSourceKind {
    /// コマンドラインフラグ（--foo など）
    CliFlag,
    /// 環境変数
    Env,
    /// プロジェクトローカルの設定ファイル（例: .aish/config.toml）
    ProjectFile,
    /// ユーザー設定ファイル（例: ~/.config/aish/config.toml）
    UserFile,
    /// ビルトインのデフォルト値
    Default,
}

/// 1 つの設定値がどこから来たかの情報
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigSource {
    pub kind: ConfigSourceKind,
    /// 具体的な識別子（env 名 / パス / フラグ名など）
    ///
    /// 例:
    /// - \"AISH_EGRESS_SENSITIVE_ACTION\"
    /// - \".aish/config.toml\"
    /// - \"~/.config/aish/config.toml\"
    /// - \"--policy.egress-sensitive-action\"
    pub ref_id: String,
}
