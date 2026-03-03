use serde::{Deserialize, Serialize};

/// ツールが持つケイパビリティ（policy 判定の入力）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolCapability {
    /// ファイル読み取り（パスは glob / prefix ベースのヒント）
    FsRead { paths: Vec<String> },
    /// ファイル書き込み
    FsWrite { paths: Vec<String> },
    /// プロセス実行（許可されたプログラムの allowlist）
    Exec { allowlist: Vec<String> },
    /// ネットワーク利用の可否（将来拡張用）
    Network { allow: bool },
    /// LLM などへのデータ送信の絶対上限（文字数）
    DataEgress { max_chars: usize },
}
