// src/swarm/mod.rs
// 🐝 蜂群模块 — 多 Agent 通信与协作
//
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

pub mod agents;
pub mod heartbeat;
pub mod orchestrator;
pub mod registry;
pub mod rpc;
pub mod transport;

pub mod pool;
pub mod task;
pub mod workflow;

// 重新导出核心类型
pub use heartbeat::HeartbeatMonitor;
pub use orchestrator::SwarmOrchestrator;
pub use registry::{
    AgentInfo, AgentRegistration, AgentStatus, AgentType, CapabilityManifest, SwarmRegistry,
};
pub use rpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, SwarmMethod};
pub use task::{SwarmTask, TaskPriority, TaskResult, TaskStatus};
pub use transport::{UdsClient, UdsServer};
