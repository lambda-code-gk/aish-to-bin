//! LLM 呼び出し・ストリーム変換の標準アダプタ

pub(crate) mod llm_completion;
pub(crate) mod llm_event_stream_factory;

#[cfg(test)]
pub(crate) mod stub_llm;

pub(crate) use llm_completion::StdLlmCompletion;
pub(crate) use llm_event_stream_factory::StdLlmEventStreamFactory;
