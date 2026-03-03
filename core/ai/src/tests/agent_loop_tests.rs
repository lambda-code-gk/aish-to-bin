//! AgentLoop（外側）の v1.2.1 テスト（Stub QueryLoopRunner で検証）

use std::sync::{Arc, Mutex};

use crate::usecase::agent_judge::{AgentJudge, AgentJudgeInput, AgentVerdict};
use crate::usecase::agent_loop::{AgentLoop, AgentLoopConfig, AgentLoopOutcome};
use crate::usecase::query_loop::{QueryLoopOutcome, QueryLoopRunner};
use common::error::Error;
use common::msg::Msg;
use serde_json::json;

#[derive(Clone)]
enum Step {
    Done { text: String, add_tool_result: bool },
}

#[derive(Clone)]
struct ScriptedQueryLoop {
    script: Arc<Mutex<Vec<Step>>>,
}

impl ScriptedQueryLoop {
    fn new(script: Arc<Mutex<Vec<Step>>>) -> Self {
        Self { script }
    }
}

impl QueryLoopRunner for ScriptedQueryLoop {
    fn run_until_done(
        &mut self,
        messages: &[Msg],
        _max_turns: usize,
        _max_additional_tool_calls: usize,
    ) -> Result<QueryLoopOutcome, Error> {
        let step = {
            let mut g = self.script.lock().unwrap();
            if g.is_empty() {
                return Ok(QueryLoopOutcome::Done(
                    messages.to_vec(),
                    "no more steps".to_string(),
                ));
            }
            g.remove(0)
        };

        match step {
            Step::Done {
                text,
                add_tool_result,
            } => {
                let mut out = messages.to_vec();
                if add_tool_result {
                    out.push(Msg::tool_result(
                        "call-1",
                        "run_shell",
                        json!({"stdout":"ok\n","stderr":"","exit_code":0}),
                    ));
                }
                Ok(QueryLoopOutcome::Done(out, text))
            }
        }
    }
}

struct RetryOnceJudge {
    called: Mutex<bool>,
}

impl RetryOnceJudge {
    fn new() -> Self {
        Self {
            called: Mutex::new(false),
        }
    }
}

impl AgentJudge for RetryOnceJudge {
    fn judge(&self, _input: &AgentJudgeInput) -> Result<AgentVerdict, common::error::Error> {
        let mut g = self.called.lock().unwrap();
        if !*g {
            *g = true;
            return Ok(AgentVerdict::Retry {
                followup: "[AISH_INTERNAL] retry_for_completion_v1\n...".to_string(),
            });
        }
        Ok(AgentVerdict::Done)
    }
}

fn count_marker(msgs: &[Msg], marker: &str) -> usize {
    msgs.iter()
        .filter(|m| matches!(m, Msg::System(s) if s.contains(marker)))
        .count()
}

fn count_tool_results(msgs: &[Msg]) -> usize {
    msgs.iter()
        .filter(|m| matches!(m, Msg::ToolResult { .. }))
        .count()
}

#[test]
fn agent_loop_retries_once_and_executes() {
    let script = Arc::new(Mutex::new(vec![
        Step::Done {
            text: "```sh\ncurl ... | voicevox ...\n```".to_string(),
            add_tool_result: false,
        },
        Step::Done {
            text: "再生しました".to_string(),
            add_tool_result: true,
        },
    ]));

    let mut calls = 0usize;
    let mut make = || {
        calls += 1;
        ScriptedQueryLoop::new(Arc::clone(&script))
    };

    let initial = vec![Msg::user("ニュース取得して読み上げて")];
    let judge = RetryOnceJudge::new();

    let out = AgentLoop::run(
        &mut make,
        &judge,
        &initial,
        AgentLoopConfig {
            max_queries: 2,
            max_turns: 1,
            max_additional_tool_calls: 0,
        },
    )
    .unwrap();

    let marker = "[AISH_INTERNAL] retry_for_completion_v1";

    match out {
        AgentLoopOutcome::Done(msgs, text) => {
            assert_eq!(calls, 2);
            assert_eq!(text, "再生しました");
            assert_eq!(count_marker(&msgs, marker), 1);
            assert!(count_tool_results(&msgs) >= 1);
        }
        _ => panic!("expected Done"),
    }
}

// 以降のテストは旧 Heuristic ベースの挙動に依存していたため、
// v1.3 では AgentJudge を Stub 化した上での retry 動作のみを検証する。
