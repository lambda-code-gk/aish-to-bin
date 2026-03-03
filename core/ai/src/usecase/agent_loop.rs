//! AgentLoop: QueryLoop を束ねる外側ループ
//!
//! - 1 回の LLM ↔ tool 反復は QueryLoop に委譲
//! - AgentLoop は「何回まで QueryLoop を回すか」「足りなければ押し込むか」を判断する

use common::error::Error;
use common::msg::Msg;

use crate::domain::AgentMode;
use crate::usecase::query_loop::{count_tool_results, QueryLoopOutcome, QueryLoopRunner};

#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    /// Agent のモード（Act/Plan/Auto）
    pub agent_mode: AgentMode,
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

                    // v1.2: shape ベースの判定で「手順提示で止まった」とみなせば 1 回だけ押し込む
                    if i + 1 < cfg.max_queries && should_retry_v2(&cfg, tool_delta, &last_text) {
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

fn should_retry_v2(cfg: &AgentLoopConfig, tool_delta: usize, assistant_text: &str) -> bool {
    // plan モードでは押し込まない
    if matches!(cfg.agent_mode, AgentMode::Plan) {
        return false;
    }
    // 直近の QueryLoop でツールが動いていれば押し込まない
    if tool_delta > 0 {
        return false;
    }
    let t = assistant_text.trim();
    if t.is_empty() {
        return false;
    }

    // 質問や追加入力待ちっぽければ押し込まない（NeedUserInput 相当）
    if looks_like_question_or_need_user_input(t) {
        return false;
    }

    // コマンド列っぽい shape なら「手順提示で止まった」とみなす
    looks_like_command_block(t)
}

fn strip_fenced_code_blocks(s: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;

    for line in s.lines() {
        let l = line.trim_start();
        if l.starts_with("```") || l.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn last_non_empty_line(s: &str) -> Option<&str> {
    s.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim())
}

fn looks_like_question_or_need_user_input(assistant_text: &str) -> bool {
    let outside = strip_fenced_code_blocks(assistant_text);
    let last = match last_non_empty_line(&outside) {
        Some(l) => l,
        None => return false,
    };

    if last.ends_with('?') || last.ends_with('？') {
        return true;
    }

    if last.ends_with(':') || last.ends_with('：') {
        let core = last.trim_end_matches(|c| c == ':' || c == '：').trim();
        if !core.is_empty()
            && core.chars().count() <= 24
            && !core.chars().any(|c| c.is_whitespace())
        {
            return true;
        }
    }

    false
}

fn looks_like_command_block(s: &str) -> bool {
    let t = s.trim();
    if t.contains("```") || t.contains("~~~") {
        return true;
    }

    let mut shell_prompt_lines = 0usize;
    let mut pipe_like_lines = 0usize;
    for line in t.lines() {
        let l = line.trim_start();
        if l.starts_with("$ ") || l.starts_with("> ") || l.starts_with("PS>") {
            shell_prompt_lines += 1;
        }
        if l.contains("&&") || l.contains(" | ") {
            pipe_like_lines += 1;
        }
    }
    shell_prompt_lines >= 1 || pipe_like_lines >= 2
}

fn inject_internal_followup(messages: &mut Vec<Msg>) {
    // 同じ internal followup を多重挿入しない
    let marker = "[AISH_INTERNAL] retry_for_completion_v1";
    if messages
        .iter()
        .any(|m| matches!(m, Msg::System(s) if s.contains(marker)))
    {
        return;
    }
    messages.push(Msg::system(format!(
        "{marker}\nobjective: complete the user's request end-to-end\nrequirements:\n  - do not stop at suggested commands only\n  - prefer using tools to execute and verify\n  - if critical information is missing, ask exactly one clarification question\n"
    )));
}
