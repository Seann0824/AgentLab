// src/tools/dag_tools/status.rs
// pipeline_status 工具 — 查看 Pipeline 执行状态

use crate::tools::dag_tools::store;
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// pipeline_status 工具
///
/// 查询已注册 Pipeline 或已执行引擎的状态。
pub struct PipelineStatus;

impl Tool for PipelineStatus {
    fn name(&self) -> &str {
        "pipeline_status"
    }

    fn description(&self) -> &str {
        "查看已注册 Pipeline 或执行引擎的当前状态。需要提供 pipeline_id。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "pipeline_status",
                "description": "查看 Pipeline 执行状态",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pipeline_id": {
                            "type": "string",
                            "description": "Pipeline ID"
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

impl PipelineStatus {
    fn execute_inner(&self, args: serde_json::Value) -> ToolEvent {
        let pipeline_id = match args.get("pipeline_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolEvent::Err("缺少 pipeline_id 参数".to_string()),
        };

        // 尝试从引擎存储中获取状态
        if let Ok(engine) = store::get_engine(pipeline_id) {
            let summary = engine.status_summary();

            // 构建详细节点信息
            let nodes_detail: serde_json::Map<String, serde_json::Value> = engine.nodes.iter()
                .map(|(id, instance)| {
                    let detail = serde_json::json!({
                        "node_id": instance.node_id,
                        "status": format!("{:?}", instance.status),
                        "worker_output": instance.worker_output,
                        "review_result": instance.review_result,
                        "final_output": instance.final_output,
                        "retry_count": instance.retry_count,
                        "logs": instance.logs,
                        "started_at": instance.started_at,
                        "completed_at": instance.completed_at,
                    });
                    (id.clone(), detail)
                })
                .collect();

            return ToolEvent::Done(serde_json::json!({
                "ok": true,
                "pipeline_id": pipeline_id,
                "status": summary,
                "nodes": nodes_detail,
                "events": engine.events.iter().map(|e| format!("{:?}", e)).collect::<Vec<_>>(),
            }));
        }

        // 尝试从 Pipeline 存储中获取定义
        if let Ok(pipeline) = store::get_pipeline(pipeline_id) {
            return ToolEvent::Done(serde_json::json!({
                "ok": true,
                "pipeline_id": pipeline_id,
                "status": "registered",
                "description": pipeline.description,
                "nodes": pipeline.nodes.len(),
                "edges": pipeline.edges.len(),
                "config": {
                    "max_concurrency": pipeline.config.max_concurrency,
                    "max_review_retries": pipeline.config.max_review_retries,
                    "node_timeout_seconds": pipeline.config.node_timeout_seconds,
                },
                "message": "Pipeline 已注册但尚未执行",
            }));
        }

        ToolEvent::Err(format!("Pipeline '{}' 未找到", pipeline_id))
    }
}
