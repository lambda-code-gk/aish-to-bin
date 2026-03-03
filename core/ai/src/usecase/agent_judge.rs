use common::error::Error;
use serde::Deserialize;
use std::sync::Arc;

use crate::ports::outbound::LlmCompletion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentVerdict {
    Done,
    Retry { followup: String },
    NeedUserInput,
    Blocked { reason: String },
}

#[derive(Debug, Clone)]
pub struct AgentJudgeInput<'a> {
    pub root_user: Option<&'a str>,
    pub assistant_text: &'a str,
    pub tool_delta: usize,
}

pub trait AgentJudge: Send + Sync {
    fn judge(&self, input: &AgentJudgeInput) -> Result<AgentVerdict, Error>;
}

// --- PlanJudge

pub struct PlanJudge;

impl AgentJudge for PlanJudge {
    fn judge(&self, _input: &AgentJudgeInput) -> Result<AgentVerdict, Error> {
        Ok(AgentVerdict::Done)
    }
}

// --- HeuristicJudge（言語非依存、形状ベース）

pub struct HeuristicJudge;

impl HeuristicJudge {
    pub fn new() -> Self {
        Self
    }
}

impl AgentJudge for HeuristicJudge {
    fn judge(&self, input: &AgentJudgeInput) -> Result<AgentVerdict, Error> {
        // ツールが動いているなら基本 Done
        if input.tool_delta > 0 {
            return Ok(AgentVerdict::Done);
        }
        let t = input.assistant_text.trim();
        if t.is_empty() {
            return Ok(AgentVerdict::Done);
        }
        // 質問/入力待ちっぽいなら retry しない
        if looks_like_question_or_need_user_input(t) {
            return Ok(AgentVerdict::NeedUserInput);
        }
        // 形状が「手順/コマンド提示で止まった」なら retry
        if looks_like_command_block(t) {
            return Ok(AgentVerdict::Retry {
                followup: default_followup(),
            });
        }
        Ok(AgentVerdict::Done)
    }
}

fn default_followup() -> String {
    let marker = "[AISH_INTERNAL] retry_for_completion_v1";
    format!(
        "{marker}\nobjective: complete the user's request end-to-end\nrequirements:\n  - do not stop at suggested commands only\n  - prefer using tools to execute and verify\n  - if critical information is missing, ask exactly one clarification question\n"
    )
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
    s.lines().rev().find(|l| !l.trim().is_empty()).map(|l| l.trim())
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
        // 空白無しパイプも拾う（誤検知は fence で防ぐ）
        if l.contains("&&") || l.contains('|') {
            pipe_like_lines += 1;
        }
    }
    shell_prompt_lines >= 1 || pipe_like_lines >= 2
}

// --- LlmJudge（JSON判定）

pub struct LlmJudge {
    llm: Arc<dyn LlmCompletion>,
}

impl LlmJudge {
    pub fn new(llm: Arc<dyn LlmCompletion>) -> Self {
        Self { llm }
    }
}

#[derive(Debug, Deserialize)]
struct JudgeJson {
    verdict: String, // done|retry|need_user_input|blocked
    #[serde(default)]
    followup: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

impl AgentJudge for LlmJudge {
    fn judge(&self, input: &AgentJudgeInput) -> Result<AgentVerdict, Error> {
        let system = r#"
You are a strict evaluator for an automation agent.
Return ONLY valid JSON matching:
{"verdict":"done|retry|need_user_input|blocked","followup":string?,"reason":string?}
Rules:
- If the assistant only suggested commands or steps but did not execute them, verdict=retry and include a followup instruction that forces tool execution and verification.
- If the assistant is asking the user for missing critical info, verdict=need_user_input.
- If blocked by permissions/policy/environment, verdict=blocked with reason.
- Otherwise verdict=done.
No extra text.
"#;

        let payload = serde_json::json!({
            "root_user": input.root_user,
            "assistant_text": input.assistant_text,
            "tool_delta": input.tool_delta,
        })
        .to_string();

        let raw = self.llm.complete(Some(system), &payload)?;
        let json_str = extract_json_object(&raw).ok_or_else(|| {
            Error::provider("llm_judge: invalid json").with_context(raw.clone())
        })?;
        let parsed: JudgeJson = serde_json::from_str(&json_str).map_err(|e| {
            Error::provider(format!("llm_judge: parse failed: {e}")).with_context(raw)
        })?;

        let v = parsed.verdict.as_str();
        Ok(match v {
            "retry" => AgentVerdict::Retry {
                followup: parsed.followup.unwrap_or_else(default_followup),
            },
            "need_user_input" => AgentVerdict::NeedUserInput,
            "blocked" => AgentVerdict::Blocked {
                reason: parsed.reason.unwrap_or_else(|| "blocked".to_string()),
            },
            _ => AgentVerdict::Done,
        })
    }
}

fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(s[start..=end].to_string())
}

// --- CompositeJudge（Auto用：Heuristic→曖昧ならLLM）

pub struct CompositeJudge {
    pub heuristic: HeuristicJudge,
    pub llm: LlmJudge,
}

impl AgentJudge for CompositeJudge {
    fn judge(&self, input: &AgentJudgeInput) -> Result<AgentVerdict, Error> {
        let h = self.heuristic.judge(input)?;
        match h {
            // retry/need_user_input/blocked は heuristic で確定してよい
            AgentVerdict::Retry { .. }
            | AgentVerdict::NeedUserInput
            | AgentVerdict::Blocked { .. } => Ok(h),

            // “Done” だが tool_delta==0 のときは曖昧なので LLM で確認
            AgentVerdict::Done if input.tool_delta == 0 => {
                // LLM が壊れても heuristic 結果にフォールバック
                match self.llm.judge(input) {
                    Ok(v) => Ok(v),
                    Err(_) => Ok(AgentVerdict::Done),
                }
            }
            _ => Ok(h),
        }
    }
}

