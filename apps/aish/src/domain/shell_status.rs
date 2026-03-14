//! shell attachment と console log 周辺の表示用値型

use crate::domain::{JobListEntry, ShellAttachment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellStatusSnapshot {
    pub session_id: String,
    pub attachment: Option<ShellAttachment>,
    pub console_exists: bool,
    pub console_bytes: Option<u64>,
    pub part_file_count: usize,
    pub latest_part_file: Option<String>,
    pub mute_flag_exists: bool,
    pub pending_input_exists: bool,
    pub prompt_suggestion_exists: bool,
    pub persisted_job_count: usize,
    pub latest_job: Option<JobListEntry>,
}
