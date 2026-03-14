//! Library entry for ai: run from env args or from given args (used by bins/aish-cli).

mod adapter;
pub mod backend_api;
mod backend_core;
mod backend_dispatch;
mod cli;
mod cli_entry;
mod domain;
mod ports;
mod usecase;
mod wiring;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use cli_entry::Runner;
pub use cli_entry::{run, run_with_args};
