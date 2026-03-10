use std::path::PathBuf;

/// package.toml から読み取った package のメタデータ
///
/// Phase F では install / remote / version solver は扱わず、
/// ローカルに存在する package を Task / Skill / Prompt / Memory の
/// 供給元として扱うための最小情報のみを持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    /// package 名（ディレクトリ名と一致していることが多いが、厳密にはそうでなくてもよい）
    pub name: String,
    /// package 自体のバージョン（文字列。セマンティックバージョンを強制しない）
    pub version: Option<String>,
    /// 短い説明文
    pub description: Option<String>,
    /// package ルートディレクトリ（package.toml が存在するディレクトリ）
    pub root_dir: PathBuf,
    /// デフォルトで起動する Task 名（省略可）
    pub default_task: Option<String>,
    /// system prompt hook のパス（package ルートからの相対パスを解決済みにした絶対パス）
    pub system_hook: Option<PathBuf>,
    /// この package が主に扱う memory topics
    pub memory_topics: Vec<String>,
    /// この package の想定する aish バージョン互換条件（例: ">=0.1.0"）
    ///
    /// Phase F では厳密判定せず、読めた場合は provenance 情報として保持するだけにとどめる。
    pub compat_aish: Option<String>,
}

