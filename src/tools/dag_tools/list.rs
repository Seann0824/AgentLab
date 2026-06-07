// src/tools/dag_tools/list.rs
// pipeline_list 工具 — 列出所有已注册的 Pipeline

use crate::tools::dag_tools::store;
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// pipeline_list 工具
///
/// 列出所有已注册的 Pipeline 及其状态。
pub struct PipelineList;

impl Tool for PipelineList {
    fn name(&self) -> &str {
        "pipeline_list"
    }

    fn description(&self) -> &str {
        "列出所有已注册的 Pipeline 和当前执行引擎。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "pipeline_list",
                "description": "列出所有已注册的 Pipeline",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        })
    }

    fn execute(&self, _args: serde_json::Value) -> ToolStream {
        let result = self.execute_inner();
        Box::pin(futures_util::stream::iter(vec![result]))
    }
}

impl PipelineList {
    fn execute_inner(&self) -> ToolEvent {
        let pipelines = match store::list_pipelines() {
            Ok(p) => p,
            Err(e) => return ToolEvent::Err(format!("获取 Pipeline 列表失败: {}", e)),
        };

        let list: Vec<serde_json::Value> = pipelines
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "description": p.description,
                    "nodes": p.nodes.len(),
                    "edges": p.edges.len(),
                    "max_concurrency": p.config.max_concurrency,
                })
            })
            .collect();

        ToolEvent::Done(serde_json::json!({
            "ok": true,
            "pipelines": list,
            "count": list.len(),
        }))
    }
}
