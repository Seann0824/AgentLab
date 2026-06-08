// src/tools/agent_tools.rs
// 🛠 Agent 专用任务工具 — 为每个子 Agent 类型提供独立的调度工具
//
// 将 5 种子 Agent（Coder/Researcher/Verifier/General/Memory）注册为独立工具，
// 这样 Main Agent（Orchestrator）可以直接按名称调用，无需手动指定 agent_type。
//
// 与 dispatch_task 的区别：
// - dispatch_task: 通用派发工具，需要 LLM 指定 agent_type 参数
// - coder_task / researcher_task / ... : 专有工具，agent_type 已预配置

use std::sync::Arc;

use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::swarm::orchestrator::SwarmOrchestrator;
use crate::swarm::registry::{AgentType, SwarmRegistry};
use crate::tools::dispatch_task::dispatch_to_agent;
use crate::tools::types::{Tool, ToolStream};

/// Agent 专用任务工具 — 为某个子 Agent 类型提供独立的调度工具
pub struct AgentTaskTool {
    pub agent_type: AgentType,
    pub tool_name: &'static str,
    pub tool_description: &'static str,
    pub param_description: &'static str,
    pub orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    pub registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
}

impl Tool for AgentTaskTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn description(&self) -> &str {
        self.tool_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.tool_name,
                "description": self.tool_description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "task_description": {
                            "type": "string",
                            "description": self.param_description
                        },
                        "task_params": {
                            "type": "object",
                            "description": "可选的任务参数（JSON 格式）",
                            "default": {}
                        },
                        "timeout_seconds": {
                            "type": "integer",
                            "description": "等待 Agent 响应的超时秒数（默认 120）",
                            "default": 120
                        }
                    },
                    "required": ["task_description"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let task_description = args["task_description"].as_str().unwrap_or("").to_string();
        let task_params = args
            .get("task_params")
            .cloned()
            .unwrap_or(serde_json::json!({}));
        let timeout_secs = args["timeout_seconds"].as_u64().unwrap_or(120);
        let agent_type_str = self.agent_type.as_str().to_string();

        let (tx, rx) = mpsc::channel(1);
        let agent_type = self.agent_type.clone();
        dispatch_to_agent(
            agent_type,
            &agent_type_str,
            &task_description,
            task_params,
            timeout_secs,
            self.orchestrator.clone(),
            self.registry.clone(),
            tx,
        );
        Box::pin(ReceiverStream::new(rx))
    }
}

/// 创建 Coder Agent 工具实例
pub fn make_coder_task(
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
) -> AgentTaskTool {
    AgentTaskTool {
        agent_type: AgentType::Coder,
        tool_name: "coder_task",
        tool_description: "默认用于代码实现/重构/生成/局部修改的 Coder Agent 调度工具。\
            支持 read_file/edit_file/generate_code/review_code 等编码操作。\
            非平凡编码任务应优先委派给它，再由 Orchestrator 复核和整合结果。",
        param_description: "编码任务描述，告诉 Coder Agent 需要生成或修改什么代码",
        orchestrator,
        registry,
    }
}

/// 创建 Researcher Agent 工具实例
pub fn make_researcher_task(
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
) -> AgentTaskTool {
    AgentTaskTool {
        agent_type: AgentType::Researcher,
        tool_name: "researcher_task",
        tool_description: "默认用于代码库/架构/技术方案调查的 Researcher Agent 调度工具。\
            支持 read_file/search_code/analyze_codebase/generate_report/compare_approaches \
            等调研操作。跨文件分析和方案比较应优先委派给它。",
        param_description: "调研任务描述，告诉 Researcher Agent 需要调研什么",
        orchestrator,
        registry,
    }
}

/// 创建 Verifier Agent 工具实例
pub fn make_verifier_task(
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
) -> AgentTaskTool {
    AgentTaskTool {
        agent_type: AgentType::Verifier,
        tool_name: "verifier_task",
        tool_description: "默认用于编译、测试、回归和错误分析的 Verifier Agent 调度工具。\
            支持 cargo check / cargo test 等编译测试验证。\
            代码修改后应优先委派给它做验证。",
        param_description: "验证任务描述，告诉 Verifier Agent 需要验证什么",
        orchestrator,
        registry,
    }
}

/// 创建 General Agent 工具实例
pub fn make_general_task(
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
) -> AgentTaskTool {
    AgentTaskTool {
        agent_type: AgentType::General,
        tool_name: "general_task",
        tool_description: "默认用于低风险通用子任务的 General Agent 调度工具。\
            适合不需要 Coder/Researcher/Verifier/Memory 专业能力的一般任务处理。",
        param_description: "通用任务描述，告诉 General Agent 需要做什么",
        orchestrator,
        registry,
    }
}

/// 创建 Memory Agent 工具实例
pub fn make_memory_task(
    orchestrator: Arc<TokioMutex<SwarmOrchestrator>>,
    registry: Option<Arc<TokioMutex<SwarmRegistry>>>,
) -> AgentTaskTool {
    AgentTaskTool {
        agent_type: AgentType::Memory,
        tool_name: "memory_task",
        tool_description: "默认用于持久化记忆检索、保存、整理和统计的 Memory Agent 调度工具。\
            适合需要主动管理跨会话知识的场景。",
        param_description: "记忆管理任务描述，告诉 Memory Agent 需要做什么",
        orchestrator,
        registry,
    }
}
