// src/tools/dag_tools/execute.rs
// pipeline_execute 工具 — 执行一个 Pipeline
//
// [Phase 4] 实现真正的 DAG 并行执行引擎：
// 1. 获取全局 DAGContext（model + tool_manager）
// 2. 创建 DAGEngine 进行节点调度
// 3. 使用 tokio::spawn 并行执行就绪节点
// 4. 使用 NodeRuntime::execute_node() 进行真正的 LLM Worker/Reviewer 执行

use std::time::Duration;

use crate::dag::engine::DAGEngine;
use crate::dag::runtime::NodeRuntime;
use crate::dag::types::{NodeStatus, PipelineStatus};
use crate::tools::dag_tools::store;
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// pipeline_execute 工具
///
/// 执行已注册的 Pipeline。
/// 使用 DAGEngine 调度 + NodeRuntime 真正执行 LLM Worker/Reviewer 流程。
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
        let pipeline_id = match args.get("pipeline_id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                return Box::pin(futures_util::stream::iter(vec![
                    ToolEvent::Err("缺少 pipeline_id 参数".to_string())
                ]));
            }
        };

        // 使用 stream::once 将异步执行转换为流
        let future = Box::pin(Self::run_pipeline(pipeline_id));
        Box::pin(futures_util::stream::once(future))
    }
}

impl PipelineExecute {
    /// 真正异步执行 Pipeline
    async fn run_pipeline(pipeline_id: String) -> ToolEvent {
        // 1. 获取全局 DAG 上下文（含 model + tool_manager）
        let ctx = match store::get_dag_context() {
            Ok(c) => c,
            Err(e) => return ToolEvent::Err(format!("DAG 上下文未初始化: {}", e)),
        };

        // 2. 创建引擎
        let mut engine = match store::create_engine(&pipeline_id) {
            Ok(e) => e,
            Err(e) => return ToolEvent::Err(format!("创建引擎失败: {}", e)),
        };

        engine.status = PipelineStatus::Running;

        // 3. 主调度循环
        loop {
            // 检查是否所有节点都已终态
            if engine.all_terminal() {
                break;
            }

            // 获取就绪节点（考虑了 max_concurrency 限制）
            let ready = engine.ready_nodes();

            if ready.is_empty() {
                // 没有就绪节点但 Pipeline 未完成 → 等待
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }

            // 4. 并行执行就绪节点
            let mut handles = Vec::new();
            for node_id in ready {
                // 查找节点定义
                let node_def = engine.pipeline.nodes.iter()
                    .find(|n| n.id == node_id)
                    .cloned();

                let Some(node_def) = node_def else {
                    // 节点未找到 — 标记失败
                    engine.on_node_failed(&node_id, format!("节点 '{}' 未在 Pipeline 定义中找到", node_id));
                    continue;
                };

                // 标记节点为 Working 状态
                if let Some(instance) = engine.nodes.get_mut(&node_id) {
                    instance.transition_to(NodeStatus::Working);
                }

                // 获取输入数据
                let input = engine.nodes.get(&node_id)
                    .and_then(|n| n.input.clone())
                    .unwrap_or(serde_json::json!({}));

                let ctx = ctx.clone();
                let handle = tokio::spawn(async move {
                    let runtime = NodeRuntime;
                    runtime.execute_node(&ctx, &node_def, input, 3).await
                });
                handles.push((node_id, handle));
            }

            // 5. 等待所有并行节点完成
            for (node_id, handle) in handles {
                match handle.await {
                    Ok(Ok(output)) => {
                        engine.on_node_completed(&node_id, output);
                    }
                    Ok(Err(e)) => {
                        engine.on_node_failed(&node_id, format!("{}", e));
                    }
                    Err(e) => {
                        engine.on_node_failed(&node_id, format!("任务崩溃: {}", e));
                    }
                }
            }
        }

        // 6. 获取最终摘要
        let summary = engine.status_summary();

        // 7. 保存引擎状态
        if let Err(e) = store::save_engine(engine) {
            return ToolEvent::Err(format!("保存引擎状态失败: {}", e));
        }

        ToolEvent::Done(serde_json::json!({
            "ok": true,
            "message": format!("Pipeline '{}' 执行完成", pipeline_id),
            "summary": summary,
        }))
    }
}
