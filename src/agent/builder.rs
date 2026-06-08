use std::sync::Arc;

use tokio::sync::Mutex;

use super::default_tools::default_tool_manager;
use super::{Agent, AgentConfig};
use crate::cli::CommandRegistry;
use crate::context::ContextManager;
use crate::goal::GoalRegistry;
use crate::memory::MemoryManager;
use crate::model::ModelManager;
use crate::session::SessionManager;
use crate::swarm::orchestrator::SwarmOrchestrator;
use crate::swarm::registry::{AgentType, SwarmRegistry};
use crate::task::TaskManager;
use crate::tools::agent_tools::{make_coder_task, make_general_task, make_memory_task, make_researcher_task, make_verifier_task};
use crate::tools::dispatch_task::DispatchTask;
use crate::tools::memory_tools::{
    MemoryForgetTool, MemorySaveTool, MemorySearchTool, MemoryStatsTool,
};
use crate::tools::ToolManager;

/// AgentBuilder — 链式构建 Agent
pub struct AgentBuilder {
    model_manager: Option<ModelManager>,
    tool_manager: Option<ToolManager>,
    config: AgentConfig,
    current_dir: String,
    memory_manager: Option<MemoryManager>,
    swarm_registry: Option<SwarmRegistry>,
    /// Swarm Orchestrator 共享引用（用于 dispatch_task 工具）
    swarm_orchestrator: Option<Arc<Mutex<SwarmOrchestrator>>>,
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self {
            model_manager: None,
            tool_manager: None,
            config: AgentConfig::default(),
            current_dir: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .display()
                .to_string(),
            memory_manager: None,
            swarm_registry: None,
            swarm_orchestrator: None,
        }
    }
}

impl AgentBuilder {
    /// 创建新的 AgentBuilder
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置模型管理器（支持多模型注册与切换）
    pub fn model_manager(mut self, mm: ModelManager) -> Self {
        self.model_manager = Some(mm);
        self
    }

    /// 设置工具管理器
    pub fn tool_manager(mut self, tm: ToolManager) -> Self {
        self.tool_manager = Some(tm);
        self
    }

    /// 设置 Agent 配置
    pub fn config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// 设置当前工作目录
    pub fn current_dir(mut self, dir: impl Into<String>) -> Self {
        self.current_dir = dir.into();
        self
    }

    /// 设置 MemoryManager（持久化记忆）
    pub fn memory_manager(mut self, mm: MemoryManager) -> Self {
        self.memory_manager = Some(mm);
        self
    }

    /// 设置 SwarmRegistry（蜂群注册表）
    pub fn swarm_registry(mut self, registry: SwarmRegistry) -> Self {
        self.swarm_registry = Some(registry);
        self
    }

    /// 设置 Agent 类型（用于渲染不同身份提示词）
    pub fn agent_type(mut self, agent_type: AgentType) -> Self {
        self.config.agent_type = agent_type;
        self
    }

    /// 设置 Swarm Orchestrator（用于派发任务到子 Agent）
    pub fn swarm_orchestrator(mut self, orch: Arc<Mutex<SwarmOrchestrator>>) -> Self {
        self.swarm_orchestrator = Some(orch);
        self
    }

    /// 构建 Agent
    pub fn build(self) -> anyhow::Result<Agent> {
        let model_manager = self.model_manager.ok_or_else(|| {
            anyhow::anyhow!("ModelManager is required. Call .model_manager(mm) to set it.")
        })?;

        // ⭐ 初始化 MemoryManager（持久化记忆）
        let memory_manager = self.memory_manager.unwrap_or_else(|| {
            MemoryManager::new_mock(std::path::PathBuf::from(&self.current_dir))
        });
        let memory_manager = Arc::new(Mutex::new(memory_manager));

        // ⭐ 构建工具管理器（注册 memory 工具）
        let mut tool_manager = self.tool_manager.unwrap_or_else(default_tool_manager);
        tool_manager.register_tool(Box::new(MemorySaveTool {
            memory_manager: memory_manager.clone(),
        }));
        tool_manager.register_tool(Box::new(MemorySearchTool {
            memory_manager: memory_manager.clone(),
        }));
        tool_manager.register_tool(Box::new(MemoryForgetTool {
            memory_manager: memory_manager.clone(),
        }));
        tool_manager.register_tool(Box::new(MemoryStatsTool {
            memory_manager: memory_manager.clone(),
        }));

        // ⭐ 如果提供了 SwarmRegistry，替换默认的 SwarmCtl 工具
        if let Some(ref registry) = self.swarm_registry {
            let swarm_ctl = crate::tools::swarm_ctl::SwarmCtl::new(Some(registry.clone()));
            tool_manager.register_tool(Box::new(swarm_ctl));
        }

        // ⭐ 如果提供了 SwarmOrchestrator，注册 dispatch_task 工具
        if let Some(ref orch) = self.swarm_orchestrator {
            let dispatch = DispatchTask {
                orchestrator: orch.clone(),
                registry: self.swarm_registry.as_ref().map(|r| {
                    // 从注册表获取 Arc 共享引用（通过 Orchestrator 获取）
                    // dispatch_task 内部会通过 orch 获取 registry
                    Arc::new(Mutex::new(r.clone()))
                }),
            };
            tool_manager.register_tool(Box::new(dispatch));

            // ⭐ 注册 Agent 专用任务工具（每个子 Agent 类型一个独立工具）
            let registry = self.swarm_registry.as_ref().map(|r| Arc::new(Mutex::new(r.clone())));
            tool_manager.register_tool(Box::new(make_coder_task(orch.clone(), registry.clone())));
            tool_manager.register_tool(Box::new(make_researcher_task(orch.clone(), registry.clone())));
            tool_manager.register_tool(Box::new(make_verifier_task(orch.clone(), registry.clone())));
            tool_manager.register_tool(Box::new(make_general_task(orch.clone(), registry.clone())));
            tool_manager.register_tool(Box::new(make_memory_task(orch.clone(), registry.clone())));
        }

        let strategy = self.config.to_strategy();

        // ⭐ 初始化 GoalRegistry
        let mut goal_manager = GoalRegistry::new(&self.current_dir);
        let _ = goal_manager.load_all();

        Ok(Agent {
            config: self.config,
            model_manager,
            tool_manager,
            context_manager: ContextManager::new("".to_string(), strategy),
            goal_manager,
            task_manager: TaskManager::new(&self.current_dir),
            session_manager: SessionManager::new(&self.current_dir, &self.current_dir),
            command_registry: CommandRegistry::new(),
            current_dir: self.current_dir,
            memory_manager,
            swarm_registry: self.swarm_registry,
            swarm_orchestrator: self.swarm_orchestrator,
        })
    }
}
