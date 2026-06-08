// src/tools/generate_tool/mod.rs
//
// GenerateTool — 工具脚手架生成器
// 允许 Agent 通过描述自动生成一个新的工具模板代码，实现「自我进化」。
//
// 功能：
//   输入工具名、描述、参数列表，自动生成完整工具代码
//   并注册到项目中的 tools/mod.rs

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

pub struct GenerateTool {
    project_root: String,
}

impl GenerateTool {
    pub fn new(project_root: &str) -> Self {
        Self {
            project_root: project_root.to_string(),
        }
    }
}

impl Tool for GenerateTool {
    fn name(&self) -> &str {
        "generate_tool"
    }

    fn description(&self) -> &str {
        "生成一个新工具（Tool）的脚手架代码。根据提供的工具名、描述和参数定义，自动创建完整的 Rust 源文件并注册到工具系统。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "generate_tool",
                "description": self.description(),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "tool_name": {
                            "type": "string",
                            "description": "工具名（snake_case，如 my_tool）将用作文件名和注册名"
                        },
                        "description": {
                            "type": "string",
                            "description": "工具功能描述，将写入代码注释和 description() 方法"
                        },
                        "params": {
                            "type": "array",
                            "description": "参数列表，每个参数包含 name, type, description, required",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string", "description": "参数名" },
                                    "type": { "type": "string", "description": "参数类型: string/number/boolean/array/object" },
                                    "description": { "type": "string", "description": "参数说明" },
                                    "required": { "type": "boolean", "description": "是否必填" }
                                },
                                "required": ["name", "type", "description", "required"]
                            }
                        }
                    },
                    "required": ["tool_name", "description", "params"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let project_root = self.project_root.clone();
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let tool_name = match args["tool_name"].as_str() {
                Some(name) => name.to_string(),
                None => {
                    let _ = tx.send(ToolEvent::Err("tool_name is required".to_string())).await;
                    return;
                }
            };

            let description = match args["description"].as_str() {
                Some(d) => d.to_string(),
                None => {
                    let _ = tx.send(ToolEvent::Err("description is required".to_string())).await;
                    return;
                }
            };

            let params = match args["params"].as_array() {
                Some(arr) => arr.clone(),
                None => {
                    let _ = tx.send(ToolEvent::Err("params must be an array".to_string())).await;
                    return;
                }
            };

            // 验证工具名格式 (snake_case, 字母数字下划线)
            let valid_name: bool = tool_name.chars().all(|c| c.is_alphanumeric() || c == '_');
            if !valid_name || tool_name.is_empty() {
                let _ = tx.send(ToolEvent::Err(
                    format!("Invalid tool name '{}'. Use snake_case: letters, numbers, underscores only.", tool_name)
                )).await;
                return;
            }

            // === 生成工具代码 ===
            let struct_name = to_pascal_case(&tool_name);
            let tool_dir = PathBuf::from(&project_root).join("src").join("tools").join(&tool_name);
            let tool_file = tool_dir.join("mod.rs");

            // 生成 JSON Schema properties
            let mut properties_json = String::new();
            let mut required_json = String::new();
            for (i, param) in params.iter().enumerate() {
                let pname = param["name"].as_str().unwrap_or("param");
                let ptype = param["type"].as_str().unwrap_or("string");
                let pdesc = param["description"].as_str().unwrap_or("");
                let required = param["required"].as_bool().unwrap_or(false);

                // Map type to JSON schema type
                let schema_type = match ptype {
                    "number" => "number",
                    "boolean" => "boolean",
                    "array" => "array",
                    "object" => "object",
                    _ => "string",
                };

                if i > 0 {
                    properties_json.push_str(",\n");
                }
                properties_json.push_str(&format!(
                    r#"                        "{}": {{
                            "type": "{}",
                            "description": "{}"
                        }}"#,
                    pname, schema_type, pdesc
                ));

                if required {
                    if !required_json.is_empty() {
                        required_json.push_str(", ");
                    }
                    required_json.push_str(&format!("\"{}\"", pname));
                }
            }

            // 生成 Rust 字段定义
            let mut rust_fields = String::new();
            for param in &params {
                let pname = param["name"].as_str().unwrap_or("param");
                let ptype = param["type"].as_str().unwrap_or("string");
                let rust_type = match ptype {
                    "number" => "f64",
                    "boolean" => "bool",
                    "array" => "Vec<String>",
                    "object" => "serde_json::Value",
                    _ => "String",
                };
                rust_fields.push_str(&format!(
                    "        let {}: {} = args[\"{}\"]\n",
                    pname, rust_type, pname
                ));
                rust_fields.push_str(&match ptype {
                    "number" => format!("            .as_f64().unwrap_or(0.0);\n"),
                    "boolean" => format!("            .as_bool().unwrap_or(false);\n"),
                    "array" => format!(
                        "            .as_array()\n            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())\n            .unwrap_or_default();\n"
                    ),
                    "object" => format!("            .clone();\n"),
                    _ => format!("            .as_str().unwrap_or(\"\").to_string();\n"),
                });
            }

            let code = format!(r#"// src/tools/{tool_dir_name}/mod.rs
//
// {struct_name} — {description}
//
// 此文件由 generate_tool 自动生成
// 生成时间: {timestamp}

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{{Tool, ToolEvent, ToolStream}};

pub struct {struct_name};

impl Tool for {struct_name} {{
    fn name(&self) -> &str {{
        "{tool_name}"
    }}

    fn description(&self) -> &str {{
        "{description}"
    }}

    fn parameters_schema(&self) -> serde_json::Value {{
        serde_json::json!({{
            "type": "function",
            "function": {{
                "name": "{tool_name}",
                "description": self.description(),
                "parameters": {{
                    "type": "object",
                    "properties": {{
{properties_json}
                    }},
                    "required": [{required_json}],
                    "additionalProperties": false
                }}
            }}
        }})
    }}

    fn execute(&self, args: serde_json::Value) -> ToolStream {{
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {{
{rust_fields}
            // TODO: 实现工具逻辑
            let result = serde_json::json!({{
                "success": true,
                "message": format!("{tool_name} executed successfully"),
                "note": "This is auto-generated scaffolding. Implement the actual logic here."
            }});

            let _ = tx.send(ToolEvent::Done(result)).await;
        }});

        Box::pin(ReceiverStream::new(rx))
    }}
}}
"#,
                tool_dir_name = tool_name,
                struct_name = struct_name,
                description = description,
                timestamp = simple_timestamp(),
                tool_name = tool_name,
                properties_json = properties_json,
                required_json = required_json,
                rust_fields = rust_fields,
            );

            // 创建目录并写入文件
            match tokio::fs::create_dir_all(&tool_dir).await {
                Ok(_) => {},
                Err(e) => {
                    let _ = tx.send(ToolEvent::Err(format!("Failed to create directory: {}", e))).await;
                    return;
                }
            }

            match tokio::fs::write(&tool_file, &code).await {
                Ok(_) => {},
                Err(e) => {
                    let _ = tx.send(ToolEvent::Err(format!("Failed to write file: {}", e))).await;
                    return;
                }
            }

            // 更新 src/tools/mod.rs 添加模块声明
            let mod_path = PathBuf::from(&project_root).join("src").join("tools").join("mod.rs");
            match tokio::fs::read_to_string(&mod_path).await {
                Ok(content) => {
                    let mod_decl = format!("pub mod {};", tool_name);
                    if content.contains(&mod_decl) {
                        // 已存在，跳过
                    } else {
                        // 在 types 之后、最后一个 pub mod 之前插入
                        let new_content = if let Some(pos) = content.rfind("pub mod ") {
                            // 找到最后一个 pub mod 声明，在其后插入
                            let insert_pos = content[pos..].find('\n').map(|p| pos + p + 1).unwrap_or(content.len());
                            let mut updated = content.clone();
                            updated.insert_str(insert_pos, &format!("pub mod {};\n", tool_name));
                            updated
                        } else {
                            // fallback: 追加到最后
                            format!("{}\npub mod {};\n", content, tool_name)
                        };

                        match tokio::fs::write(&mod_path, &new_content).await {
                            Ok(_) => {
                                let msg = format!(
                                    "✅ Tool '{}' scaffolding created successfully!\n\nFile created: {}\n\nNext steps:\n1. Implement the tool logic in the `execute()` method (search for TODO)\n2. Register in agent.rs: `tool_manager.register_tool(Box::new({}));`\n3. Run `cargo check` to verify",
                                    tool_name,
                                    tool_file.to_string_lossy(),
                                    struct_name
                                );
                                let result = serde_json::json!({
                                    "success": true,
                                    "tool_name": tool_name,
                                    "file_path": tool_file.to_string_lossy(),
                                    "message": msg,
                                    "next_steps": [
                                        format!("Implement logic in src/tools/{0}/mod.rs execute() method", tool_name),
                                        format!("Register in agent.rs: tool_manager.register_tool(Box::new({0}));", struct_name),
                                        "Run `cargo check` to verify compilation"
                                    ]
                                });
                                let _ = tx.send(ToolEvent::Done(result)).await;
                            },
                            Err(e) => {
                                let _ = tx.send(ToolEvent::Err(format!("File created but failed to update mod.rs: {}", e))).await;
                            }
                        }
                    }
                },
                Err(e) => {
                    let _ = tx.send(ToolEvent::Err(format!("Failed to read mod.rs: {}", e))).await;
                }
            }
        });

        Box::pin(ReceiverStream::new(rx))
    }
}

/// 将 snake_case 转为 PascalCase
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// 生成简单的时间戳字符串（不含 chrono 依赖）
fn simple_timestamp() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let total_minutes = secs / 60;
    let hours = (total_minutes / 60) % 24;
    let minutes = total_minutes % 60;
    let days = secs / 86400;
    let years = 1970 + (days / 365);
    let month = ((days % 365) / 30) + 1;
    let day = ((days % 365) % 30) + 1;
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", years, month.min(12), day.min(28), hours, minutes, secs % 60)
}
