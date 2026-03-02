//! AgentLoop: QueryLoop を束ねる外側ループ
//!
//! - 1 回の LLM ↔ tool 反復は QueryLoop に委譲
//! - AgentLoop は「何回まで QueryLoop を回すか」「足りなければ押し込むか」を判断する

use common::error::Error;
use common::msg::Msg;

use crate::usecase::query_loop::{QueryLoop, QueryLoopOutcome, count_tool_results};

#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    /// QueryLoop を何回まで回すか（v1 は 2 を推奨：押し込み 1 回）
    pub max_queries: usize,
    /// 1 QueryLoop あたりの上限（既存の AI_MAX_TURNS 相当）
    pub max_turns: usize,
    /// 1 QueryLoop あたりの上限（既存の AI_MAX_TOOL_CALLS 相当）
    pub max_additional_tool_calls: usize,
}

#[derive(Debug, Clone)]
pub enum AgentLoopOutcome {
    Done(Vec<Msg>, String),
    ReachedLimit(Vec<Msg>, String),
}

pub struct AgentLoop;

impl AgentLoop {
    pub fn run<F>(
        mut make_query_loop: F,
        initial_messages: &[Msg],
        cfg: AgentLoopConfig,
    ) -> Result<AgentLoopOutcome, Error>
    where
        F: FnMut() -> QueryLoop,
    {
        let mut messages = initial_messages.to_vec();
        let mut last_text = String::new();

        // “最初のユーザー要求”を拾う（後で Judge に使う）
        let root_user = messages.iter().rev().find_map(|m| match m {
            Msg::User(s) => Some(s.clone()),
            _ => None,
        });

        for i in 0..cfg.max_queries {
            let before_tool_results = count_tool_results(&messages);

            let mut ql = make_query_loop();
            let out = ql.run_until_done(&messages, cfg.max_turns, cfg.max_additional_tool_calls)?;

            match out {
                QueryLoopOutcome::ReachedLimit(msgs, text) => {
                    return Ok(AgentLoopOutcome::ReachedLimit(msgs, text));
                }
                QueryLoopOutcome::Done(msgs, text) => {
                    messages = msgs;
                    last_text = text;

                    let after_tool_results = count_tool_results(&messages);
                    let tool_delta = after_tool_results.saturating_sub(before_tool_results);

                    // v1 Judge: “手順提示で止まった”なら 1 回だけ押し込む
                    if i + 1 < cfg.max_queries
                        && should_retry_v1(root_user.as_deref(), tool_delta, &last_text)
                    {
                        inject_internal_followup(&mut messages);
                        continue;
                    }

                    return Ok(AgentLoopOutcome::Done(messages, last_text));
                }
            }
        }

        Ok(AgentLoopOutcome::ReachedLimit(messages, last_text))
    }
}

fn should_retry_v1(root_user: Option<&str>, tool_delta: usize, assistant_text: &str) -> bool {
    if tool_delta > 0 {
        return false;
    }
    let user = root_user.unwrap_or("");
    if !looks_like_action_request(user) {
        return false;
    }
    looks_like_command_suggestion(assistant_text)
}

fn looks_like_action_request(s: &str) -> bool {
    let t = s.trim();
    t.contains("して") || t.contains("してください") || t.contains("やって")
        || t.contains("取得") || t.contains("読み上げ") || t.contains("再生")
}

fn looks_like_command_suggestion(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains("```")
        || lower.contains("$ ")
        || lower.contains("curl ")
        || lower.contains("jq ")
        || lower.contains("voicevox")
        || lower.contains("run_shell")
}

fn inject_internal_followup(messages: &mut Vec<Msg>) {
    // 同じ internal followup を多重挿入しない
    let marker = "[AISH_INTERNAL] retry_for_completion_v1";
    if messages.iter().any(|m| matches!(m, Msg::User(s) if s.contains(marker))) {
        return;
    }
    messages.push(Msg::User(format!(
        "{marker}\nまだ要求は未達です。手順提示だけで止まらず、ツール（例: run_shell 等）を使って実行し、成功を確認してから結果を報告してください。"
    )));
}

