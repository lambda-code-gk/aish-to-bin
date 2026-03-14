//! shell attachment の値オブジェクト

use crate::domain::ShellStorageLayout;
use serde::{Deserialize, Serialize};

pub const SHELL_ATTACHMENT_FILENAME: &str = "shell_attachment.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShellAttachmentStatus {
    Attached,
    Detached,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShellJobLinkMode {
    SessionEvents,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShellPartTrackingMode {
    LayoutOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShellAttachment {
    pub v: u32,
    pub status: ShellAttachmentStatus,
    pub pid: Option<u32>,
    pub attached_at: Option<String>,
    pub detached_at: Option<String>,
    pub updated_at: String,
    pub muted: bool,
    pub console_path: String,
    pub pending_input_path: String,
    pub prompt_suggestion_path: String,
    pub mute_flag_path: String,
    pub part_file_prefix: String,
    pub job_link_mode: ShellJobLinkMode,
    pub part_file_tracking_mode: ShellPartTrackingMode,
}

impl ShellAttachment {
    pub fn attached(pid: u32, muted: bool, now: &str) -> Self {
        let storage = ShellStorageLayout::default();
        Self {
            v: 1,
            status: ShellAttachmentStatus::Attached,
            pid: Some(pid),
            attached_at: Some(now.to_string()),
            detached_at: None,
            updated_at: now.to_string(),
            muted,
            console_path: storage.console_path,
            pending_input_path: storage.pending_input_path,
            prompt_suggestion_path: storage.prompt_suggestion_path,
            mute_flag_path: storage.mute_flag_path,
            part_file_prefix: storage.part_file_prefix,
            job_link_mode: ShellJobLinkMode::SessionEvents,
            part_file_tracking_mode: ShellPartTrackingMode::LayoutOnly,
        }
    }

    pub fn detached_from(existing: Option<Self>, now: &str) -> Self {
        match existing {
            Some(mut current) => {
                current.status = ShellAttachmentStatus::Detached;
                current.pid = None;
                current.detached_at = Some(now.to_string());
                current.updated_at = now.to_string();
                current
            }
            None => {
                let storage = ShellStorageLayout::default();
                Self {
                    v: 1,
                    status: ShellAttachmentStatus::Detached,
                    pid: None,
                    attached_at: None,
                    detached_at: Some(now.to_string()),
                    updated_at: now.to_string(),
                    muted: false,
                    console_path: storage.console_path,
                    pending_input_path: storage.pending_input_path,
                    prompt_suggestion_path: storage.prompt_suggestion_path,
                    mute_flag_path: storage.mute_flag_path,
                    part_file_prefix: storage.part_file_prefix,
                    job_link_mode: ShellJobLinkMode::SessionEvents,
                    part_file_tracking_mode: ShellPartTrackingMode::LayoutOnly,
                }
            }
        }
    }

    pub fn with_muted(mut self, muted: bool, now: &str) -> Self {
        self.muted = muted;
        self.updated_at = now.to_string();
        self
    }
}
