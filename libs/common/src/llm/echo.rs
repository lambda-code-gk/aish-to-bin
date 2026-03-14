//! Echoプロバイダの実装
//!
//! このプロバイダは実際にLLM APIを呼び出さず、固定応答や疑似イベントを返します。
//! 表示は行わず、デバッグやテスト用に使用します。

use crate::error::Error;
use crate::llm::events::{FinishReason, LlmEvent};
use crate::llm::provider::{LlmProvider, Message};
use crate::tool::ToolDef;
use serde_json::{json, Value};
use std::thread;
use std::time::Duration;

/// Echoプロバイダ
pub struct EchoProvider;

impl EchoProvider {
    /// 新しいEchoプロバイダを作成
    pub fn new() -> Self {
        Self
    }
}

impl LlmProvider for EchoProvider {
    fn name(&self) -> &str {
        "echo"
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, Error> {
        // ダミーのレスポンスを返す（実際のAPI呼び出しは行わない）
        Ok(json!({ "echo": request_json }).to_string())
    }

    fn parse_response_text(&self, _response_json: &str) -> Result<Option<String>, Error> {
        // Echoプロバイダは常に固定のメッセージを返す
        Ok(Some(
            "[Echo Provider] Query received (no actual LLM call made)".to_string(),
        ))
    }

    fn check_tool_calls(&self, _response_json: &str) -> Result<bool, Error> {
        // ストリーム側でツール呼び出しをシミュレートするため、ここでは未使用
        Ok(false)
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
        _tools: Option<&[ToolDef]>,
    ) -> Result<Value, Error> {
        // シンプルなペイロードを生成
        let mut payload = json!({
            "query": query,
        });

        if let Some(system) = system_instruction {
            payload["system_instruction"] = json!(system);
        }

        if !history.is_empty() {
            let history_json: Vec<Value> = history
                .iter()
                .map(|msg| {
                    json!({
                        "role": msg.role,
                        "content": msg.content
                    })
                })
                .collect();
            payload["history"] = json!(history_json);
        }

        Ok(payload)
    }

    fn make_http_streaming_request(
        &self,
        _request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        let text = "[Echo Provider] This is a simulated streaming response from the echo provider. It displays text chunk by chunk to demonstrate the streaming capability.";

        for word in text.split_whitespace() {
            callback(word)?;
            callback(" ")?;
            thread::sleep(Duration::from_millis(50));
        }

        Ok(())
    }

    /// ストリームを LlmEvent に正規化。ツール定義がある場合、「call:ツール名」や「call:ツール名 {...}」でツール呼び出しをシミュレートする。
    fn stream_events(
        &self,
        request_json: &str,
        tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let payload: Value = match serde_json::from_str(request_json) {
            Ok(p) => p,
            Err(_) => return self.stream_events_text_only(callback, "", 0, None),
        };
        let query = payload["query"].as_str().unwrap_or("").trim();
        let history = payload["history"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        let system_instruction = payload.get("system_instruction").and_then(|v| v.as_str());

        // 直近がツール結果なら、テキスト応答を返すか、ループテスト用にツール呼び出しを続ける
        let last_is_tool = history
            .last()
            .and_then(|m| m.get("role").and_then(|r| r.as_str()))
            .map(|r| r == "tool")
            .unwrap_or(false);
        if last_is_tool {
            let tool_result_count = history
                .iter()
                .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("tool"))
                .count();
            // ツール結果のあと、何回までツール呼び出しを返すか。未設定時は 2（2 ターンで ReachedLimit を手動確認しやすい）
            let loop_test_limit = std::env::var("AI_ECHO_LOOP_TEST")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(2);
            let tool_names: Vec<&str> = tools
                .map(|t| t.iter().map(|d| d.name.as_str()).collect())
                .unwrap_or_default();
            let first_tool = tool_names.first().copied().unwrap_or("run_shell");

            if tool_result_count < loop_test_limit {
                let step = tool_result_count + 1;
                let call_id = format!("echo_loop_{}", step);
                let args = json!({ "command": format!("echo loop step {}", step) });
                let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                callback(LlmEvent::ToolCallBegin {
                    call_id: call_id.clone(),
                    name: first_tool.to_string(),
                    thought_signature: None,
                })?;
                callback(LlmEvent::ToolCallArgsDelta {
                    call_id: call_id.clone(),
                    json_fragment: args_json,
                })?;
                callback(LlmEvent::ToolCallEnd { call_id })?;
                callback(LlmEvent::Completed {
                    finish: FinishReason::ToolCalls,
                })?;
                return Ok(());
            }

            let result_preview = history
                .last()
                .and_then(|m| m.get("content").and_then(|c| c.as_str()))
                .unwrap_or("{}");
            let msg = format!(
                "[Echo Provider] Tool result was received. Proceeding with: {}",
                if result_preview.len() > 60 {
                    format!(
                        "{}...",
                        &result_preview[..result_preview.floor_char_boundary(60)]
                    )
                } else {
                    result_preview.to_string()
                }
            );
            for word in msg.split_whitespace() {
                callback(LlmEvent::TextDelta(word.to_string()))?;
                callback(LlmEvent::TextDelta(" ".to_string()))?;
                thread::sleep(Duration::from_millis(30));
            }
            callback(LlmEvent::Completed {
                finish: FinishReason::Stop,
            })?;
            return Ok(());
        }

        // ツール一覧から名前を取得
        let tool_names: Vec<&str> = tools
            .map(|t| t.iter().map(|d| d.name.as_str()).collect())
            .unwrap_or_default();

        // 「call:名前」または「call: 名前」で始まる場合はツール呼び出しをシミュレート
        for name in &tool_names {
            let prefix = format!("call:{}", name);
            let prefix_sp = format!("call: {}", name);
            let is_call = query.eq_ignore_ascii_case(&prefix)
                || query.starts_with(&prefix_sp)
                || (query.len() >= prefix.len()
                    && query
                        .get(..prefix.len())
                        .map(|s| s.eq_ignore_ascii_case(&prefix))
                        == Some(true));
            if is_call {
                let args_str = query
                    .strip_prefix(&prefix)
                    .or_else(|| query.strip_prefix(&prefix_sp))
                    .or_else(|| {
                        if query.len() >= prefix.len()
                            && query
                                .get(..prefix.len())
                                .map(|s| s.eq_ignore_ascii_case(&prefix))
                                == Some(true)
                        {
                            Some(&query[prefix.len()..])
                        } else {
                            None
                        }
                    })
                    .unwrap_or("")
                    .trim();
                let args: Value = if args_str.is_empty() {
                    // run_shell は "command" 必須のため、引数なし時は安全なデフォルトを渡す
                    if *name == "run_shell" {
                        json!({ "command": "echo ok" })
                    } else {
                        json!({})
                    }
                } else if let Ok(v) = serde_json::from_str(args_str) {
                    v
                } else {
                    json!({ "message": args_str, "input": args_str })
                };
                let call_id = format!("echo_call_{}", name);
                callback(LlmEvent::ToolCallBegin {
                    call_id: call_id.clone(),
                    name: (*name).to_string(),
                    thought_signature: None, // Echo provider doesn't use thought signatures
                })?;
                let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                callback(LlmEvent::ToolCallArgsDelta {
                    call_id: call_id.clone(),
                    json_fragment: args_json,
                })?;
                callback(LlmEvent::ToolCallEnd { call_id })?;
                callback(LlmEvent::Completed {
                    finish: FinishReason::ToolCalls,
                })?;
                return Ok(());
            }
        }

        // 通常のテキスト応答
        let history_count = history.len();
        self.stream_events_text_only(callback, query, history_count, system_instruction)
    }
}

impl EchoProvider {
    fn stream_events_text_only(
        &self,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
        query: &str,
        history_count: usize,
        system_instruction: Option<&str>,
    ) -> Result<(), Error> {
        let mut text = format!("[Echo Provider] Query: user message: {}\n\n", query);
        if history_count > 0 {
            text.push_str(&format!(
                "[Echo Provider] History: {} messages\n",
                history_count
            ));
        }
        if let Some(system_instruction) = system_instruction {
            text.push_str(&format!(
                "[Echo Provider] System instruction: {}\n",
                system_instruction
            ));
        }
        text.push('\n');
        text.push_str(&format!(
            "[Echo Provider] 採用された履歴件数: {} 件。\n\n",
            history_count
        ));
        text.push_str(
            "[Echo Provider] This is a simulated streaming response from the echo provider. It displays text chunk by chunk to demonstrate the streaming capability.",
        );
        callback(LlmEvent::TextDelta(text))?;
        thread::sleep(Duration::from_millis(50));
        callback(LlmEvent::Completed {
            finish: FinishReason::Stop,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echo_provider_name() {
        let provider = EchoProvider::new();
        assert_eq!(provider.name(), "echo");
    }

    #[test]
    fn test_echo_provider_make_request_payload() {
        let provider = EchoProvider::new();
        let payload = provider
            .make_request_payload("Hello", None, &[], None)
            .unwrap();
        assert_eq!(payload["query"], "Hello");
    }

    #[test]
    fn test_echo_provider_make_request_payload_with_system() {
        let provider = EchoProvider::new();
        let payload = provider
            .make_request_payload("Hello", Some("You are helpful"), &[], None)
            .unwrap();
        assert_eq!(payload["query"], "Hello");
        assert_eq!(payload["system_instruction"], "You are helpful");
    }

    #[test]
    fn test_echo_provider_make_request_payload_with_history() {
        let provider = EchoProvider::new();
        let history = vec![Message::user("Hi"), Message::assistant("Hello!")];
        let payload = provider
            .make_request_payload("How are you?", None, &history, None)
            .unwrap();
        assert_eq!(payload["query"], "How are you?");
        assert!(payload["history"].is_array());
        assert_eq!(payload["history"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_echo_provider_parse_response_text() {
        let provider = EchoProvider::new();
        let result = provider.parse_response_text("{}").unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().contains("Echo Provider"));
    }

    #[test]
    fn test_echo_provider_check_tool_calls() {
        let provider = EchoProvider::new();
        let result = provider.check_tool_calls("{}").unwrap();
        assert_eq!(result, false);
    }

    #[test]
    fn test_echo_provider_stream_events_text_only_includes_query_history_and_system() {
        let provider = EchoProvider::new();
        let payload = provider
            .make_request_payload(
                "say hello",
                Some("You are helpful"),
                &[Message::user("first"), Message::assistant("second")],
                None,
            )
            .unwrap();
        let request_json = serde_json::to_string(&payload).unwrap();
        let mut events = Vec::new();
        provider
            .stream_events(&request_json, None, &mut |ev| {
                events.push(ev);
                Ok(())
            })
            .unwrap();
        assert!(matches!(
            events.last(),
            Some(LlmEvent::Completed {
                finish: FinishReason::Stop
            })
        ));
        let text = events
            .iter()
            .filter_map(|ev| match ev {
                LlmEvent::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(text.contains("[Echo Provider] Query: user message: say hello"));
        assert!(text.contains("[Echo Provider] History: 2 messages"));
        assert!(text.contains("[Echo Provider] System instruction: You are helpful"));
        assert!(text.contains("[Echo Provider] 採用された履歴件数: 2 件。"));
        assert!(text.contains("simulated streaming response"));
    }
}
