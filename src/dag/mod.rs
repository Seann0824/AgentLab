// src/dag/mod.rs
// DAG 任务编排系统 — 模块入口
//
// 设计文档: docs/designs/dag-task-orchestration.md
//
// 基于有向无环图（DAG）的任务编排系统。
// 每个任务节点内部由一个 Worker Agent 和一个 Reviewer Agent 组成，
// 审核通过后自动转发到下游节点。

pub mod types;
pub mod edge;
pub mod node;
pub mod pipeline;
pub mod engine;
pub mod dataflow;
pub mod runtime;
pub mod utils;
pub mod node_internal;
pub mod persistence;
pub mod event_bus;
pub mod logger;

// 公共导出 — 最常用的类型
pub use types::{
    NodeStatus, PipelineStatus, ReviewCriteria, CheckResult, ReviewResult,
    InputMode, OutputMode, MergeStrategy, DAGError, DAGResult,
    WorkerOutput, ReviewOutput, ReviewMode, NodeResult,
    NodeLog, LogLevel, LogSource, DAGEvent,
};
pub use edge::{EdgeDef, DataMapping};
pub use node::{NodeDef, NodeInstance};
pub use pipeline::{PipelineDef, PipelineConfig};
pub use engine::DAGEngine;
pub use dataflow::DataFlowManager;
