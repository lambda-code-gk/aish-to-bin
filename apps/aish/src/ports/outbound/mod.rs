//! Outbound ポート: アプリが外界（シェル起動等）を使うための trait

pub mod memory_repository;
pub mod reviewed_history_reader;
pub mod shell_attachment_store;
pub mod shell_runner;

pub use memory_repository::MemoryRepository;
pub use reviewed_history_reader::ReviewedHistoryReader;
pub use shell_attachment_store::ShellAttachmentStore;
pub use shell_runner::ShellRunner;
