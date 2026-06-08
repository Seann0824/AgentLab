// src/tools/read_tool/mod.rs
//
// ReadTool - 文件读取工具
//
// 功能：
//   - 读取文件全部内容
//   - 支持行号范围（start_line ~ end_line）
//   - 支持显示行号
//   - 大文件自动截断显示

use std::path::Path;

use tokio::{fs, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

pub struct ReadTool;

const DEFAULT_MAX_LENGTH: usize = 5000;

impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "读取文件内容，支持行号范围和行号显示。适合快速查看文件内容。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read",
                "description": "读取文件内容。支持按行号范围读取，可选显示行号。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "要读取的文件路径"
                        },
                        "start_line": {
                            "type": "integer",
                            "description": "起始行号（1-based，包含），不指定则从第1行开始",
                            "minimum": 1
                        },
                        "end_line": {
                            "type": "integer",
                            "description": "结束行号（1-based，包含），不指定则读到文件末尾",
                            "minimum": 1
                        },
                        "show_line_numbers": {
                            "type": "boolean",
                            "description": "是否显示行号，默认为 true",
                            "default": true
                        },
                        "max_length": {
                            "type": "integer",
                            "description": "最大输出字符数，超出的部分会被截断。默认 5000",
                            "default": 5000,
                            "minimum": 100
                        }
                    },
                    "required": ["file_path"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let result = execute_read(args).await;
            let event = match result {
                Ok(output) => ToolEvent::Done(serde_json::json!(output)),
                Err(err) => ToolEvent::Err(err),
            };
            let _ = tx.send(event).await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}

#[derive(Debug, serde::Serialize)]
struct ReadOutput {
    file_path: String,
    total_lines: usize,
    start_line: usize,
    end_line: usize,
    content: String,
    truncated: bool,
    line_count: usize,
}

async fn execute_read(args: serde_json::Value) -> Result<ReadOutput, String> {
    let file_path = args["file_path"]
        .as_str()
        .ok_or_else(|| "file_path is required".to_string())?
        .to_string();

    let show_line_numbers = args["show_line_numbers"].as_bool().unwrap_or(true);
    let max_length = args["max_length"]
        .as_u64()
        .unwrap_or(DEFAULT_MAX_LENGTH as u64) as usize;

    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(format!("文件不存在: {}", file_path));
    }

    let content = fs::read_to_string(path)
        .await
        .map_err(|e| format!("读取文件失败: {}", e))?;

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    let start_line = args["start_line"].as_u64().unwrap_or(1) as usize;
    let end_line = args["end_line"].as_u64().unwrap_or(total_lines as u64) as usize;

    if start_line < 1 {
        return Err("start_line 必须 >= 1".to_string());
    }
    if start_line > total_lines {
        return Err(format!(
            "start_line ({}) 超出文件总行数 ({})",
            start_line, total_lines
        ));
    }
    let end_line = end_line.min(total_lines);
    if end_line < start_line {
        return Err(format!(
            "end_line ({}) 不能小于 start_line ({})",
            end_line, start_line
        ));
    }

    let selected_lines = &all_lines[(start_line - 1)..end_line];

    let mut output_lines: Vec<String> = Vec::new();
    for (i, line) in selected_lines.iter().enumerate() {
        let line_num = start_line + i;
        if show_line_numbers {
            output_lines.push(format!("{:>6} | {}", line_num, line));
        } else {
            output_lines.push(line.to_string());
        }
    }

    let mut display = output_lines.join("\n");
    let full_len = display.len();

    let truncated = display.len() > max_length;
    if truncated {
        display = display.chars().take(max_length).collect();
        display.push_str(&format!(
            "\n\n... [输出已截断，共 {} 字符，仅显示前 {} 字符]",
            full_len, max_length
        ));
    }

    Ok(ReadOutput {
        file_path,
        total_lines,
        start_line,
        end_line,
        content: display,
        truncated,
        line_count: selected_lines.len(),
    })
}
