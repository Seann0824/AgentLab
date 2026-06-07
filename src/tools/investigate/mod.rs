/// investigate 工具 — 加载错误快照排查分析
///
/// 当工具调用报错时，agent 可以调用此工具加载错误现场的上下文快照，
/// 在完整上下文中分析错误根因。

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::investigate::ErrorSnapshotManager;
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// investigate 工具
pub struct InvestigateTool {
    /// 项目根目录（用于定位 .agent/snapshots/）
    root_dir: String,
}

impl InvestigateTool {
    pub fn new(root_dir: impl Into<String>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }
}

impl Tool for InvestigateTool {
    fn name(&self) -> &str {
        "investigate"
    }

    fn description(&self) -> &str {
        "加载一个错误快照，返回报错时的完整现场上下文（错误信息 + 上下文消息 + 任务状态），用于排查分析。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "investigate",
                "description": "加载一个错误快照，返回报错时的完整现场上下文。查看所有快照用 snapshot_id='list'",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "snapshot_id": {
                            "type": "string",
                            "description": "要分析的错误快照 ID。使用 'list' 列出所有可用快照。"
                        }
                    },
                    "required": ["snapshot_id"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let snapshot_id = args["snapshot_id"].as_str().unwrap_or("").to_string();
        let root_dir = self.root_dir.clone();

        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            if snapshot_id.is_empty() {
                let _ = tx.send(ToolEvent::Err("snapshot_id is required".to_string())).await;
                return;
            }

            let manager = ErrorSnapshotManager::new(&root_dir);

            if snapshot_id == "list" {
                // 列出所有快照
                match manager.list() {
                    Ok(list) => {
                        let _ = tx.send(ToolEvent::Done(serde_json::json!({
                            "snapshots": list,
                            "count": list.len(),
                            "hint": "使用 investigate(\"snapshot_id\") 分析某个快照"
                        }))).await;
                    }
                    Err(e) => {
                        let _ = tx.send(ToolEvent::Err(format!("无法列出快照: {}", e))).await;
                    }
                }
                return;
            }

            // 加载特定快照
            match manager.load(&snapshot_id) {
                Ok(snapshot) => {
                    // 返回结构化的快照数据供 LLM 分析
                    let _ = tx.send(ToolEvent::Done(serde_json::json!({
                        "snapshot_id": snapshot.id,
                        "created_at": snapshot.created_at,
                        "error": {
                            "tool": snapshot.error.tool_name,
                            "args": snapshot.error.args,
                            "output": snapshot.error.output,
                            "exit_code": snapshot.error.exit_code,
                            "duration_ms": snapshot.error.duration_ms,
                        },
                        "context": snapshot.context,
                        "task_context": {
                            "plan": snapshot.task_context.plan,
                            "agenda": snapshot.task_context.agenda,
                            "total_messages": snapshot.task_context.total_messages,
                        },
                        "analysis_hint": "请基于以上错误信息和报错前的上下文消息，分析：1) 错误定位 2) 根因分析 3) 修复方案"
                    }))).await;
                }
                Err(e) => {
                    let _ = tx.send(ToolEvent::Err(format!(
                        "无法加载快照 '{}': {}。使用 investigate(\"list\") 查看可用快照。", snapshot_id, e
                    ))).await;
                }
            }
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
