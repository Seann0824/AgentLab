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
        tool_description: "向 Coder Agent（编码专用 Agent）派发编码任务。\
            支持 read_file/edit_file/generate_code/review_code 等编码操作。\
            适合代码生成、编辑、审查等编码密集型任务。",
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
        tool_description: "向 Researcher Agent（技术调研专用 Agent）派发调研任务。\
            支持 read_file/search_code/analyze_codebase/generate_report/compare_approaches \
            等调研操作。适合代码库分析、架构调研、技术方案对比等任务。",
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
        tool_description: "向 Verifier Agent（验证专用 Agent）派发验证任务。\
            支持 cargo check / cargo test 等编译测试验证。\
            适合代码修改后的编译验证、测试执行和错误分析。",
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
        tool_description: "向 General Agent（通用 Agent）派发通用任务。\
            适合不需要专用 Agent 类型的一般性任务处理。",
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
        tool_description: "向 Memory Agent（记忆管理专用 Agent）派发记忆管理任务。\
            适合需要主动管理持久化记忆的场景。",
        param_description: "记忆管理任务描述，告诉 Memory Agent 需要做什么",
        orchestrator,
        registry,
    }
}
