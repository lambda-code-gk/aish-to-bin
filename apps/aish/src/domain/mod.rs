//! ドメイン型（Newtype、enum、ルール）

pub mod command;
pub mod history;
pub mod job;
pub mod memory;
pub mod session_event;
pub mod shell_attachment;
pub mod shell_status;
pub mod shell_storage;
pub use history::{HistoryGetEntry, HistoryListEntry};
pub use job::JobListEntry;
pub use memory::{MemoryEntry, MemoryListEntry};
pub use session_event::SessionEvent;
pub use shell_attachment::{
    ShellAttachment, ShellAttachmentStatus, ShellJobLinkMode, ShellPartTrackingMode,
    SHELL_ATTACHMENT_FILENAME,
};
pub use shell_status::ShellStatusSnapshot;
pub use shell_storage::{ShellStorageLayout, PENDING_INPUT_FILENAME, PROMPT_SUGGESTION_FILENAME};
