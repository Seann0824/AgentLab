// src/tools/dag_tools/build.rs
// pipeline_build 工具 — 构建并注册一个 Pipeline

use std::pin::Pin;

use futures_util::Stream;
use serde_json;

use crate::dag::node::NodeDef;
use crate::dag::edge::EdgeDef;
use crate::dag::pipeline::{PipelineDef, PipelineConfig};
use crate::dag::types::ReviewCriteria;
use crate::tools::dag_tools::store;
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// pipeline_build 工具
///
/// 通过 JSON 定义构建一个 Pipeline 并注册到系统中。
///
/// 参数格式：
/// ```json
/// {
///   "id": "my-pipeline",
///   "description": "...",
///   "nodes": [
///     {
///       "id": "node1",
///       "name": "节点1",
///       "instruction": "任务指令",
///       "review_items": ["检查项1", "检查项2"],
///       "review_guidelines": "审核指南"
///     }
///   ],
///   "edges": [
///     { "from": "node1", "to": "node2" }
///   ],
///   "config": {
///     "max_concurrency": 2,
///     "max_review_retries": 3
///   }
/// }
/// ```
pub struct PipelineBuild;

impl Tool for PipelineBuild {
    fn name(&self) -> &str {
        "pipeline_build"
    }

    fn description(&self) -> &str {
        "构建并注册一个 DAG Pipeline。接收 JSON 格式的 Pipeline 定义，包含节点和依赖关系。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "pipeline_build",
                "description": "构建并注册一个 DAG Pipeline",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pipeline_json": {
                            "type": "string",
                            "description": "Pipeline 定义的 JSON 字符串"
                        }
                    },
                    "required": ["pipeline_json"]
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let result = self.execute_inner(args);
        Box::pin(futures_util::stream::iter(vec![result]))
    }
}

impl PipelineBuild {
    fn execute_inner(&self, args: serde_json::Value) -> ToolEvent {
        // 解析参数
        let pipeline_json_str = match args.get("pipeline_json").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolEvent::Err("缺少 pipeline_json 参数".to_string()),
        };

        // 解析 JSON
        let json: serde_json::Value = match serde_json::from_str(pipeline_json_str) {
            Ok(v) => v,
            Err(e) => return ToolEvent::Err(format!("JSON 解析失败: {}", e)),
        };

        let id = json["id"].as_str().unwrap_or("unnamed");
        let description = json["description"].as_str().unwrap_or("");

        // 构建 Pipeline
        let mut pipeline = PipelineDef::new(id, description);

        // 解析节点
        if let Some(nodes) = json["nodes"].as_array() {
            for node_json in nodes {
                let node_id = node_json["id"].as_str().unwrap_or("unknown");
                let node_name = node_json["name"].as_str().unwrap_or(node_id);
                let instruction = node_json["instruction"].as_str().unwrap_or("");
                let description = node_json["description"].as_str().unwrap_or("");

                // 构建审核标准
                let mut criteria = ReviewCriteria::new();
                if let Some(items) = node_json["review_items"].as_array() {
                    for item in items {
                        if let Some(item_str) = item.as_str() {
                            criteria = criteria.check(item_str);
                        }
                    }
                }
                if let Some(guidelines) = node_json["review_guidelines"].as_str() {
                    criteria = criteria.guidelines(guidelines);
                }

                let node = NodeDef::new(node_id, node_name)
                    .description(description)
                    .worker_instruction(instruction)
                    .review_criteria(criteria);

                pipeline = pipeline.add_node(node);
            }
        }

        // 解析边
        if let Some(edges) = json["edges"].as_array() {
            for edge_json in edges {
                let from = edge_json["from"].as_str().unwrap_or("");
                let to = edge_json["to"].as_str().unwrap_or("");
                if !from.is_empty() && !to.is_empty() {
                    pipeline = pipeline.add_edge(EdgeDef::new(from, to));
                }
            }
        }

        // 解析配置
        if let Some(config_json) = json["config"].as_object() {
            let mut config = PipelineConfig::default();
            if let Some(v) = config_json.get("max_concurrency").and_then(|v| v.as_u64()) {
                config.max_concurrency = v as usize;
            }
            if let Some(v) = config_json.get("max_review_retries").and_then(|v| v.as_u64()) {
                config.max_review_retries = v as u32;
            }
            if let Some(v) = config_json.get("node_timeout_seconds").and_then(|v| v.as_u64()) {
                config.node_timeout_seconds = v;
            }
            pipeline = pipeline.config(config);
        }

        // 注册到全局存储
        match store::register_pipeline(pipeline) {
            Ok(()) => ToolEvent::Done(serde_json::json!({
                "ok": true,
                "message": format!("Pipeline '{}' 已成功注册", id),
                "pipeline_id": id
            })),
            Err(e) => ToolEvent::Err(format!("注册失败: {}", e)),
        }
    }
}
