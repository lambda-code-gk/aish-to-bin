//! 責務: daemon client と backend daemon runtime 境界を提供する。

mod client;
mod server;
mod worker;

pub use client::{
    default_socket_path, run_aish_read, run_aish_write, run_cancel_ai, run_list_active_jobs,
    run_ping, run_rebuild_derived, run_status, run_stop,
};
pub use server::{run_server, ServerHandlers};
pub use worker::{maybe_run_worker_from_env, run_worker_stdio};
