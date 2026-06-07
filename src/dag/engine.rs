// src/dag/engine.rs
// DAGEngine — 核心调度器

use std::collections::{HashMap, HashSet};

use crate::dag::edge::EdgeDef;
use crate::dag::node::{NodeDef, NodeInstance};
use crate::dag::pipeline::PipelineDef;
use crate::dag::types::{
    DAGEvent, DAGResult, MergeStrategy, NodeStatus, PipelineStatus,
};

/// DAG 引擎 — 图调度器
///
/// 负责：
/// 1. 拓扑排序 — 基于 `edges` 计算节点的执行顺序，检测环
/// 2. 状态管理 — 维护所有 `NodeInstance` 的状态
/// 3. 并行调度 — 当多个节点的依赖全部就绪时，并行派发
/// 4. 数据路由 — 节点完成后，将 `final_output` 路由到下游节点
#[derive(Debug, Clone)]
pub struct DAGEngine {
    /// Pipeline 定义
    pub pipeline: PipelineDef,
    /// 节点运行时实例
    pub nodes: HashMap<String, NodeInstance>,
    /// Pipeline 执行状态
    pub status: PipelineStatus,
    /// 拓扑排序后的节点顺序
    pub execution_order: Vec<String>,
    /// 事件记录
    pub events: Vec<DAGEvent>,
    /// Engine 启动时间戳（Unix 秒）
    pub started_at: f64,
}

impl DAGEngine {
    /// 创建新的 DAGEngine 并执行拓扑排序
    pub fn new(pipeline: PipelineDef) -> DAGResult<Self> {
        // 验证 Pipeline
        pipeline.validate()?;

        let node_ids = pipeline.node_ids();
        let execution_order = crate::dag::edge::topological_sort(&node_ids, &pipeline.edges)?;

        // 创建节点运行时实例
        let mut nodes = HashMap::new();
        for node_id in &node_ids {
            let mut instance = NodeInstance::new(node_id);
            // 如果节点没有上游（入度为 0），则设置为 Ready
            let upstream = pipeline.upstream_nodes(node_id);
            if upstream.is_empty() {
                instance.status = NodeStatus::Ready;
            }
            nodes.insert(node_id.clone(), instance);
        }

        Ok(Self {
            pipeline,
            nodes,
            status: PipelineStatus::Pending,
            execution_order,
            events: Vec::new(),
            started_at: crate::dag::utils::now_secs(),
        })
    }

    /// 获取所有可执行的节点（状态为 Ready 或 Rejected 可重试）
    pub fn ready_nodes(&self) -> Vec<String> {
        let max_concurrency = self.pipeline.config.max_concurrency;
        let running_count = self.nodes.values()
            .filter(|n| matches!(n.status, NodeStatus::Working | NodeStatus::Reviewing))
            .count();

        // 如果达到并行上限，返回空
        if running_count >= max_concurrency {
            return Vec::new();
        }

        let capacity = max_concurrency - running_count;

        self.nodes.iter()
            .filter(|(_, n)| n.is_executable())
            .take(capacity)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 检查所有节点是否已完成
    pub fn all_completed(&self) -> bool {
        self.nodes.values().all(|n| n.status.is_terminal())
    }

    /// 检查是否所有节点都已进入终态（含失败）
    pub fn all_terminal(&self) -> bool {
        self.nodes.values().all(|n| n.status.is_terminal())
    }

    /// 获取当前 Pipeline 状态摘要
    pub fn status_summary(&self) -> serde_json::Value {
        let total = self.nodes.len();
        let completed = self.nodes.values().filter(|n| n.status == NodeStatus::Completed).count();
        let failed = self.nodes.values().filter(|n| n.status.is_failed()).count();
        let running = self.nodes.values().filter(|n| {
            matches!(n.status, NodeStatus::Working | NodeStatus::Reviewing)
        }).count();
        let pending = self.nodes.values().filter(|n| {
            matches!(n.status, NodeStatus::Pending | NodeStatus::Ready)
        }).count();

        serde_json::json!({
            "pipeline_id": self.pipeline.id,
            "status": format!("{:?}", self.status),
            "total_nodes": total,
            "completed": completed,
            "failed": failed,
            "running": running,
            "pending": pending,
        })
    }

    /// 获取下游节点 ID 列表
    pub fn downstream_nodes(&self, node_id: &str) -> Vec<String> {
        self.pipeline.downstream_nodes(node_id)
    }

    /// 获取上游节点 ID 列表
    pub fn upstream_nodes(&self, node_id: &str) -> Vec<String> {
        self.pipeline.upstream_nodes(node_id)
    }

    /// 检查节点的所有上游是否已完成
    pub fn all_upstream_completed(&self, node_id: &str) -> bool {
        let upstream = self.upstream_nodes(node_id);
        if upstream.is_empty() {
            return true;
        }
        upstream.iter().all(|id| {
            self.nodes.get(id).map_or(false, |n| n.status == NodeStatus::Completed)
        })
    }

    /// 当节点失败时，标记失败状态并通知下游
    pub fn on_node_failed(&mut self, node_id: &str, error: String) {
        // 标记节点失败
        if let Some(instance) = self.nodes.get_mut(node_id) {
            instance.transition_to(NodeStatus::Failed { error: error.clone() });
        }

        // 记录事件
        self.events.push(DAGEvent::NodeStatusChanged {
            node_id: node_id.to_string(),
            old_status: NodeStatus::Working,
            new_status: NodeStatus::Failed { error },
        });

        // 检查是否所有节点都已终态
        if self.all_terminal() {
            let failed_nodes: Vec<String> = self.nodes.values()
                .filter(|n| n.status.is_failed())
                .map(|n| n.node_id.clone())
                .collect();
            self.status = PipelineStatus::Failed { failed_nodes };

            let duration = crate::dag::utils::now_secs() - self.started_at;
            self.events.push(DAGEvent::PipelineFailed {
                id: self.pipeline.id.clone(),
                error: format!("节点 '{}' 执行失败", node_id),
                failed_node: node_id.to_string(),
            });
        }
    }

    /// 当节点完成时，更新下游节点的输入并重新计算 Ready 状态
    ///
    /// output 结构：
    /// ```json
    /// { "content": "...", "worker_output": "...", "review": { ... } }
    /// ```
    /// review 和 worker_output 会被提取并存储到 NodeInstance 中。
    pub fn on_node_completed(
        &mut self,
        node_id: &str,
        output: serde_json::Value,
    ) {
        // Step 1: 更新当前节点状态（提取 review 和 worker_output）
        if let Some(instance) = self.nodes.get_mut(node_id) {
            // 提取 worker_output
            if let Some(wo) = output.get("worker_output").and_then(|v| v.as_str()) {
                instance.worker_output = Some(wo.to_string());
            }
            // 提取 content（作为 fallback worker_output）
            if instance.worker_output.is_none() {
                if let Some(c) = output.get("content").and_then(|v| v.as_str()) {
                    instance.worker_output = Some(c.to_string());
                }
            }
            // 提取 review_result
            if let Some(review) = output.get("review") {
                instance.review_result = serde_json::from_value(review.clone()).ok();
            }
            instance.final_output = Some(output.clone());
            instance.transition_to(NodeStatus::Completed);
        }

        // Step 2: 记录事件
        self.events.push(DAGEvent::NodeStatusChanged {
            node_id: node_id.to_string(),
            old_status: NodeStatus::Approved,
            new_status: NodeStatus::Completed,
        });

        // Step 3: 收集下游节点需要的数据（避免借用冲突）
        let downstream = self.downstream_nodes(node_id);
        let mut downstream_updates: Vec<(String, serde_json::Value, bool)> = Vec::new();

        for downstream_id in &downstream {
            // 收集所有上游的输出
            let mut upstream_outputs: HashMap<String, serde_json::Value> = HashMap::new();
            for upstream_id in self.upstream_nodes(downstream_id) {
                if let Some(up_instance) = self.nodes.get(&upstream_id) {
                    if let Some(ref out) = up_instance.final_output {
                        upstream_outputs.insert(upstream_id, out.clone());
                    }
                }
            }
            let merged_input = serde_json::json!(upstream_outputs);
            let all_ready = self.all_upstream_completed(downstream_id);
            downstream_updates.push((downstream_id.clone(), merged_input, all_ready));
        }

        // Step 4: 更新下游节点状态（收集完成后统一操作，避免借用冲突）
        for (downstream_id, merged_input, all_ready) in &downstream_updates {
            if let Some(instance) = self.nodes.get_mut(downstream_id) {
                instance.input = Some(merged_input.clone());
                if *all_ready {
                    instance.transition_to(NodeStatus::Ready);
                }
            }
        }

        // Step 5: 检查是否所有节点都已终态
        if self.all_terminal() {
            let has_failed = self.nodes.values().any(|n| n.status.is_failed());
            if has_failed {
                let failed_nodes: Vec<String> = self.nodes.values()
                    .filter(|n| n.status.is_failed())
                    .map(|n| n.node_id.clone())
                    .collect();
                self.status = PipelineStatus::Failed { failed_nodes };
            } else {
                self.status = PipelineStatus::Completed;
            }

            let duration = crate::dag::utils::now_secs() - self.started_at;
            self.events.push(DAGEvent::PipelineCompleted {
                id: self.pipeline.id.clone(),
                total_duration_secs: duration,
                node_count: self.nodes.len(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::node::NodeDef;
    use crate::dag::edge::EdgeDef;
    use crate::dag::pipeline::PipelineDef;

    #[test]
    fn test_engine_new_simple_chain() {
        let pipeline = PipelineDef::new("test", "测试")
            .add_node(NodeDef::new("a", "A"))
            .add_node(NodeDef::new("b", "B"))
            .add_edge(EdgeDef::new("a", "b"));

        let engine = DAGEngine::new(pipeline).unwrap();
        assert_eq!(engine.nodes.len(), 2);
        // 无上游的节点 a 应处于 Ready
        assert_eq!(engine.nodes["a"].status, NodeStatus::Ready);
        // 有上游的节点 b 应处于 Pending
        assert_eq!(engine.nodes["b"].status, NodeStatus::Pending);
    }

    #[test]
    fn test_engine_new_parallel() {
        let pipeline = PipelineDef::new("parallel", "并行")
            .add_node(NodeDef::new("a", "A"))
            .add_node(NodeDef::new("b", "B"))
            .add_node(NodeDef::new("c", "C"))
            .add_edge(EdgeDef::new("a", "b"))
            .add_edge(EdgeDef::new("a", "c"));

        let engine = DAGEngine::new(pipeline).unwrap();
        assert_eq!(engine.nodes["a"].status, NodeStatus::Ready);
        assert_eq!(engine.nodes["b"].status, NodeStatus::Pending);
        assert_eq!(engine.nodes["c"].status, NodeStatus::Pending);
    }

    #[test]
    fn test_engine_ready_nodes() {
        let pipeline = PipelineDef::new("ready-test", "就绪节点测试")
            .add_node(NodeDef::new("a", "A"))
            .add_node(NodeDef::new("b", "B"))
            .add_node(NodeDef::new("c", "C"))
            .add_edge(EdgeDef::new("a", "b"))
            .add_edge(EdgeDef::new("a", "c"));

        let engine = DAGEngine::new(pipeline).unwrap();
        let ready = engine.ready_nodes();
        assert_eq!(ready, vec!["a"]);
    }

    #[test]
    fn test_engine_all_completed() {
        let pipeline = PipelineDef::new("check", "检查")
            .add_node(NodeDef::new("a", "A"));

        let mut engine = DAGEngine::new(pipeline).unwrap();
        assert!(!engine.all_completed());

        // 模拟完成
        engine.on_node_completed("a", serde_json::json!({"result": "done"}));
        assert!(engine.all_completed());
    }

    #[test]
    fn test_engine_on_node_completed_triggers_downstream() {
        let pipeline = PipelineDef::new("trigger", "触发下游")
            .add_node(NodeDef::new("a", "A"))
            .add_node(NodeDef::new("b", "B"))
            .add_edge(EdgeDef::new("a", "b"));

        let mut engine = DAGEngine::new(pipeline).unwrap();
        assert_eq!(engine.nodes["b"].status, NodeStatus::Pending);

        // a 完成后，b 应变为 Ready
        engine.on_node_completed("a", serde_json::json!({"result": "done"}));
        assert_eq!(engine.nodes["a"].status, NodeStatus::Completed);
        assert_eq!(engine.nodes["b"].status, NodeStatus::Ready);
    }

    #[test]
    fn test_engine_status_summary() {
        let pipeline = PipelineDef::new("summary", "摘要测试")
            .add_node(NodeDef::new("a", "A"))
            .add_node(NodeDef::new("b", "B"))
            .add_edge(EdgeDef::new("a", "b"));

        let engine = DAGEngine::new(pipeline).unwrap();
        let summary = engine.status_summary();
        assert_eq!(summary["pipeline_id"], "summary");
        assert_eq!(summary["total_nodes"], 2);
    }

    #[test]
    fn test_engine_all_upstream_completed() {
        let pipeline = PipelineDef::new("upstream", "上游检查")
            .add_node(NodeDef::new("a", "A"))
            .add_node(NodeDef::new("b", "B"))
            .add_node(NodeDef::new("c", "C"))
            .add_edge(EdgeDef::new("a", "c"))
            .add_edge(EdgeDef::new("b", "c"));

        let mut engine = DAGEngine::new(pipeline).unwrap();

        // c 的上游 a 和 b 未完成
        assert!(!engine.all_upstream_completed("c"));

        // a 完成后，c 的上游仍未全部完成
        engine.on_node_completed("a", serde_json::json!({}));
        assert!(!engine.all_upstream_completed("c"));

        // b 完成后，c 的上游全部完成
        engine.on_node_completed("b", serde_json::json!({}));
        assert!(engine.all_upstream_completed("c"));
    }
}
