//! 責務: ツール呼び出しの Allow/Deny/RequireApproval を決める domain ルールを束ねる façade。個別ツールの表示仕様（要約の出し方）は知らない。要約は呼び出し元が port 経由で取得する。

pub use crate::domain::{ShellAllowlistRule, ToolModeRule};
