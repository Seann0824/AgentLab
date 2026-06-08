// src/tools/edit/mod.rs
//
// EditTool - 增量文件编辑工具

mod diff;
mod file_io;
mod operations;
mod types;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

use self::operations::execute_edit;

pub struct EditTool;

impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "对文件进行增量编辑（搜索替换/插入/删除/追加），而不是全量重写。支持 search_replace / insert / delete / append 四种操作。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "edit",
                "description": "增量编辑文件内容。支持四种操作：\n\
                  1. search_replace: 搜索指定文本块并替换（最常用）\n\
                  2. insert: 在指定行号或匹配文本处插入内容\n\
                  3. delete: 删除指定行范围或匹配的文本块\n\
                  4. append: 在文件末尾追加内容\n\
                  \n\
                  【重要】search_replace 的 search 文本必须与文件中内容精确匹配（包括空格和缩进），\
                  搜索文本应当足够长以确保唯一性。\n\
                  \n\
                  所有操作都支持 dry_run=true 来预览变更而不实际修改文件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "要编辑的文件路径"
                        },
                        "operation": {
                            "type": "string",
                            "enum": ["search_replace", "insert", "delete", "append"],
                            "description": "编辑操作类型"
                        },
                        "search": {
                            "type": "string",
                            "description": "【search_replace/delete 必填】精确匹配的搜索文本。必须与文件中内容完全一致（包括空格和缩进）。建议包含足够多的上下文以确保唯一匹配。"
                        },
                        "replace": {
                            "type": "string",
                            "description": "【search_replace 必填】替换后的新文本"
                        },
                        "content": {
                            "type": "string",
                            "description": "【insert/append 必填】要插入或追加的内容"
                        },
                        "line": {
                            "type": "integer",
                            "description": "【insert】插入位置的行号（1-based）。如果不提供，则使用 after/before 配合 search 定位。",
                            "minimum": 1
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["before", "after"],
                            "description": "【insert】在匹配行之前还是之后插入，默认为 after",
                            "default": "after"
                        },
                        "line_start": {
                            "type": "integer",
                            "description": "【delete 使用行范围时】起始行号（1-based，包含）",
                            "minimum": 1
                        },
                        "line_end": {
                            "type": "integer",
                            "description": "【delete 使用行范围时】结束行号（1-based，包含）",
                            "minimum": 1
                        },
                        "dry_run": {
                            "type": "boolean",
                            "description": "预览模式，为 true 时只显示 diff 但不实际修改文件",
                            "default": false
                        }
                    },
                    "required": ["file_path", "operation"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let result = execute_edit(args).await;
            let event = match result {
                Ok(output) => ToolEvent::Done(serde_json::json!(output)),
                Err(err) => ToolEvent::Err(err),
            };
            let _ = tx.send(event).await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
