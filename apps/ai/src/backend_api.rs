//! 責務: daemon worker が使う ai backend 実行 API だけを公開する。
//!
//! フロントエンド (`ai` / `aish` / 将来の browser frontend 等) からこのモジュールを
//! 直接参照してはならない。AI 実行は必ず daemon 経由の backend プロトコル
//! (`apps/daemon/src/worker/` → `run_request`) を通じて行う。

pub use crate::backend_core::{
    emit_lifecycle, emit_lifecycle_text, run_request, BackendApprovalHandler, BackendFrameEmitter,
};
