// src/tools/dag_tools/execute.rs
// pipeline_execute 工具 — 执行一个 Pipeline

use crate::dag::engine::DAGEngine;
use crate::dag::types::NodeStatus;
use crate::tools::dag_tools::store;
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// pipeline_execute 工具
///
/// 执行已注册的 Pipeline。
/// 目前模拟 DAGEngine 的调度执行过程（在 Phase 4 中将集成真正的 Worker/Reviewer LLM 调用）。
pub struct PipelineExecute;

impl Tool for PipelineExecute {
    fn name(&self) -> &str {
        "pipeline_execute"
    }

    fn description(&self) -> &str {
        "执行已注册的 DAG Pipeline。需要提供 pipeline_id。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "pipeline_execute",
                "description": "执行一个 DAG Pipeline",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pipeline_id": {
                            "type": "string",
                            "description": "要执行的 Pipeline ID"
                        }
                    },
                    "required": ["pipeline_id"]
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let result = self.execute_inner(args);
        Box::pin(futures_util::stream::iter(vec![result]))
    }
}

impl PipelineExecute {
    fn execute_inner(&self, args: serde_json::Value) -> ToolEvent {
        let pipeline_id = match args.get("pipeline_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolEvent::Err("缺少 pipeline_id 参数".to_string()),
        };

        // 创建引擎
        let mut engine = match store::create_engine(pipeline_id) {
            Ok(e) => e,
            Err(e) => return ToolEvent::Err(format!("创建引擎失败: {}", e)),
        };

        engine.status = crate::dag::types::PipelineStatus::Running;

        // 模拟执行：按照拓扑顺序逐个标记节点完成
        let execution_order = engine.execution_order.clone();

        for node_id in &execution_order {
            // 检查该节点是否可执行
            if let Some(instance) = engine.nodes.get(node_id) {
                if instance.status != NodeStatus::Ready && instance.status != NodeStatus::Pending {
                    continue;
                }
            }

            // 检查上游是否都已完成
            if !engine.all_upstream_completed(node_id) {
                continue;
            }

            // 标记为 Working → Completed（模拟执行）
            if let Some(instance) = engine.nodes.get_mut(node_id) {
                instance.transition_to(NodeStatus::Working);
                instance.transition_to(NodeStatus::Approved);
            }

            // 触发下游更新
            engine.on_node_completed(
                node_id,
                serde_json::json!({
                    "node_id": node_id,
                    "result": format!("节点 '{}' 执行完成（模拟）", node_id),
                    "status": "completed",
                }),
            );
        }

        // 获取最终状态
        let summary = engine.status_summary();

        // 保存引擎状态
        if let Err(e) = store::save_engine(engine) {
            return ToolEvent::Err(format!("保存引擎状态失败: {}", e));
        }

        ToolEvent::Done(serde_json::json!({
            "ok": true,
            "message": format!("Pipeline '{}' 执行完成", pipeline_id),
            "summary": summary
        }))
    }
}
