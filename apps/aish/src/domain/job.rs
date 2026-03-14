//! job lifecycle 表示用の値型

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobListEntry {
    pub job_id: String,
    pub parent_job_id: Option<String>,
    pub state: String,
    pub exit_code: Option<i32>,
}
