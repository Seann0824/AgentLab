// src/swarm/mod.rs
// 🐝 蜂群模块 — 多 Agent 通信与协作
//
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

pub mod transport;
pub mod rpc;
pub mod registry;
pub mod heartbeat;
pub mod agents;
pub mod orchestrator;

pub mod pool;
pub mod workflow;

// 重新导出核心类型
pub use transport::{UdsServer, UdsClient};
pub use rpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcError, SwarmMethod};
pub use registry::{SwarmRegistry, AgentInfo, AgentStatus, AgentType};
pub use heartbeat::HeartbeatMonitor;
pub use orchestrator::SwarmOrchestrator;
