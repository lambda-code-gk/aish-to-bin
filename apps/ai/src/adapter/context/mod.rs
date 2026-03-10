//! 文脈組み立て・memory・history 選別・context pack の標準アダプタ

pub(crate) mod context_addon_selectors;
pub(crate) mod context_addon_selectors_grep;
pub(crate) mod context_addon_selectors_memory;
pub(crate) mod context_artifact_store;
pub(crate) mod context_message_builder;
pub(crate) mod context_pack_builder;
pub(crate) mod memory_context_resolver;
pub(crate) mod memory_storage;
pub(crate) mod render_memory_context;
pub(crate) mod structured_memory_repository;
pub(crate) mod reducer;
pub(crate) mod resolve_memory_dir;

pub(crate) use context_addon_selectors::ChangedFilesSnippetSelector;
pub(crate) use context_addon_selectors_grep::GrepHitsSelector;
pub(crate) use context_addon_selectors_memory::MemorySelector;
pub(crate) use context_artifact_store::StdContextArtifactStore;
#[allow(unused_imports)]
pub(crate) use context_message_builder::StdContextMessageBuilder;
#[allow(unused_imports)]
pub(crate) use context_pack_builder::{StdContextPackBuilder, StdContextPackBuilderWithAddons};
pub(crate) use memory_context_resolver::StdMemoryContextResolver;
pub(crate) use structured_memory_repository::StdStructuredMemoryRepository;
pub(crate) use reducer::{PassThroughReducer, TailWindowReducer};
pub(crate) use resolve_memory_dir::StdResolveMemoryDir;
