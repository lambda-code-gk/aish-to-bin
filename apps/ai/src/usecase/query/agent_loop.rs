//! AgentLoop: QueryLoop を束ねる外側ループ
//!
//! - 1 回の LLM ↔ tool 反復は QueryLoop に委譲
//! - AgentLoop は「何回まで QueryLoop を回すか」「足りなければ押し込むか」を判断する

use common::error::Error;
use common::msg::Msg;

use crate::usecase::agent_judge::{AgentJudge, AgentJudgeInput, AgentVerdict};
use crate::usecase::query_loop::{count_tool_results, QueryLoopOutcome, QueryLoopRunner};

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
    pub fn run<F, Q>(
        make_query_loop: &mut F,
        judge: &dyn AgentJudge,
        initial_messages: &[Msg],
        cfg: AgentLoopConfig,
    ) -> Result<AgentLoopOutcome, Error>
    where
        F: FnMut() -> Q,
        Q: QueryLoopRunner,
    {
        let mut messages = initial_messages.to_vec();
        let mut last_text = String::new();

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

                    let input = AgentJudgeInput {
                        root_user: initial_messages.iter().rev().find_map(|m| {
                            if let Msg::User(s) = m {
                                Some(s.as_str())
                            } else {
                                None
                            }
                        }),
                        assistant_text: &last_text,
                        tool_delta,
                    };

                    if i + 1 < cfg.max_queries {
                        match judge.judge(&input)? {
                            AgentVerdict::Retry { followup } => {
                                inject_followup_system(&mut messages, &followup);
                                continue;
                            }
                            _ => {}
                        }
                    }

                    return Ok(AgentLoopOutcome::Done(messages, last_text));
                }
            }
        }

        Ok(AgentLoopOutcome::ReachedLimit(messages, last_text))
    }
}

fn inject_followup_system(messages: &mut Vec<Msg>, followup: &str) {
    let marker = "[AISH_INTERNAL] retry_for_completion_v1";
    if messages
        .iter()
        .any(|m| matches!(m, Msg::System(s) if s.contains(marker)))
    {
        return;
    }
    messages.push(Msg::system(followup.to_string()));
}
