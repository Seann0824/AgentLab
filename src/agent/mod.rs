// Agent 核心 — 持有所有状态，运行主循环，支持多 Agent
//
// 设计文档: docs/designs/MULTI_AGENT_ARCHITECTURE.md

mod builder;
mod config;
mod default_tools;
mod goal_command;
mod goal_loop;
mod handle;
mod input;
mod model_command;
mod model_turn;
mod post_turn;
mod prompt;
mod recovery;
mod render;
mod runtime;
mod session_command;
mod swarm_command;

pub use builder::AgentBuilder;
pub use config::AgentConfig;
pub use handle::AgentHandle;

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::cli::CommandRegistry;
use crate::context::ContextManager;
use crate::goal::GoalRegistry;
use crate::memory::MemoryManager;
use crate::model::ModelManager;
use crate::session::SessionManager;
use crate::swarm::orchestrator::SwarmOrchestrator;
use crate::swarm::registry::SwarmRegistry;
use crate::task::TaskManager;
use crate::tools::ToolManager;

/// Agent — 持有所有状态，运行主循环
pub struct Agent {
    pub(super) config: AgentConfig,
    pub(super) model_manager: ModelManager,
    pub(super) tool_manager: ToolManager,
    pub(super) context_manager: ContextManager,
    pub(super) memory_manager: Arc<Mutex<MemoryManager>>,
    pub(super) goal_manager: GoalRegistry,
    pub(super) task_manager: TaskManager,
    pub(super) session_manager: SessionManager,
    pub(super) command_registry: CommandRegistry,
    pub(super) current_dir: String,
    pub(super) swarm_registry: Option<SwarmRegistry>,
    /// Swarm Orchestrator 共享引用（用于派发任务到子 Agent）
    pub(super) swarm_orchestrator: Option<Arc<Mutex<SwarmOrchestrator>>>,
}
