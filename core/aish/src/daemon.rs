//! 単一ライタ daemon（aishd）: Unix ソケットで append / ping を提供
//!
//! Phase10: daemon 無しでも成立（接続できなければ CLI は in-proc にフォールバック）。

#[cfg(unix)]
mod unix;

#[cfg(unix)]
#[allow(unused_imports)] // run_rebuild_derived は main.rs（sessions サブコマンド）で使用
pub use unix::{default_socket_path, run_ping, run_rebuild_derived, run_server, run_status};
