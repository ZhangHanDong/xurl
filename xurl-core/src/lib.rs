pub mod error;
pub mod incremental;
pub mod model;
pub mod process;
pub mod provider;
pub mod render;
pub mod service;
pub mod uri;

pub use error::{Result, XurlError};
pub use incremental::IncrementalReader;
pub use model::{
    ActiveSession, MessageRole, PiEntryListView, ProviderKind, ResolutionMeta, ResolvedThread,
    SubagentDetailView, SubagentInfo, SubagentListView, SubagentView, ThreadMessage, ToolCall,
};
pub use process::{discover_agent_pid, discover_agent_pids, discover_pid_for_session, AgentProcess};
pub use provider::ProviderRoots;
pub use render::{extract_tool_calls, TOOL_TYPES};
pub use service::{
    list_subagents, render_subagent_view_markdown, render_thread_head_markdown,
    render_thread_markdown, resolve_subagent_view, resolve_thread, resolve_thread_json,
};
pub use uri::ThreadUri;
