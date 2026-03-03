//! LLMドライバーとプロバイダの実装
//!
//! このモジュールは、異なるLLMプロバイダ（Gemini、GPTなど）で共通する処理を提供します。

pub mod config;
pub mod driver;
pub mod echo;
pub mod events;
pub mod factory;
pub mod gemini;
pub mod gpt;
pub mod openai_compat;
pub mod provider;
pub mod resolver;

pub use config::{ProfilesConfig, ProviderProfile, ProviderTypeKind};
pub use driver::LlmDriver;
pub use events::{FinishReason, LlmEvent};
pub use factory::{create_driver, create_provider, ProviderType};
pub use provider::LlmProvider;
pub use resolver::{
    list_available_profiles, load_profiles_config, resolve_provider, ResolvedProvider,
};
