// src/swarm/agents/coder.rs
// 💻 Code Agent — 编码 Agent
//
// Code Agent 是一个非交互式 Agent，通过 UDS 与 Orchestrator 通信。
// 职责：
// 1. 专注代码生成与修改
// 2. 多文件重构
// 3. 代码评审
// 4. 与 Verifier Agent 联动（修改→验证循环）

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use tokio::sync::Mutex as TokioMutex;
use tokio::time::interval;

use crate::swarm::heartbeat::create_heartbeat_request;
use crate::swarm::rpc::JsonRpcRequest;
use crate::swarm::transport::{UdsClient, default_socket_path};

/// Code Agent — 编码 Agent
pub struct CoderAgent {
    /// Agent ID
    agent_id: String,
    /// UDS 客户端（连接到 Orchestrator），用 Arc<Mutex> 共享给心跳任务
    client: Option<Arc<TokioMutex<UdsClient>>>,
    /// 是否正在运行
    running: bool,
    /// 项目路径
    project_path: PathBuf,
}

impl CoderAgent {
    /// 创建新的 Code Agent
    pub fn new(project_path: Option<PathBuf>) -> Self {
        Self {
            agent_id: format!("coder-{}", std::process::id()),
            client: None,
            running: false,
            project_path: project_path.unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    /// 连接到 Orchestrator
    pub async fn connect(&mut self, orchestrator_socket: Option<PathBuf>) -> Result<()> {
        let socket = orchestrator_socket.unwrap_or_else(default_socket_path);
        eprintln!("💻 Code Agent 连接到 Orchestrator @ {:?}", socket);

        let client = UdsClient::connect(&socket, &self.agent_id)
            .await
            .context(format!("无法连接到 Orchestrator (socket: {:?})", socket))?;

        eprintln!("💻 Code Agent '{}' 已注册到蜂群", self.agent_id);

        self.client = Some(Arc::new(TokioMutex::new(client)));
        Ok(())
    }

    /// 运行 Code Agent 主循环
    pub async fn run(&mut self) -> Result<()> {
        self.running = true;
        eprintln!("💻 Code Agent 主循环已启动");

        // 启动心跳任务
        let agent_id = self.agent_id.clone();
        let client_arc = self.client.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(15));
            loop {
                ticker.tick().await;
                if let Some(ref client) = client_arc {
                    let mut client = client.lock().await;
                    let hb = create_heartbeat_request(&agent_id);
                    if let Err(e) = client.send_request(&hb).await {
                        eprintln!("💻 [Heartbeat] 发送失败: {}", e);
                    }
                }
            }
        });

        // 主循环：等待处理任务
        while self.running {
            if let Some(ref client_arc) = self.client {
                let mut client = client_arc.lock().await;
                match client.read_request().await {
                    Ok(request) => {
                        let _method = request.method.clone();
                        drop(client);
                        self.handle_request(request).await;
                    }
                    Err(e) => {
                        drop(client);
                        eprintln!("💻 读取请求失败: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        Ok(())
    }

    /// 处理收到的请求
    async fn handle_request(&mut self, request: JsonRpcRequest) {
        match request.method.as_str() {
            "read_file" => {
                let file_path = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("file_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!("💻 读取文件: {}", file_path);
                let result = self.read_file(file_path).await;
                self.send_response(&request.id, result).await;
            }
            "edit_file" => {
                let file_path = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("file_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let operation = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("operation"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("search_replace");
                let search_text = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("search"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let replace_text = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("replace"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let _insert_content = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                eprintln!("💻 编辑文件: {} (操作: {})", file_path, operation);
                let result = self
                    .edit_file(file_path, operation, search_text, replace_text, _insert_content)
                    .await;
                self.send_response(&request.id, result).await;
            }
            "generate_code" => {
                let specification = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("specification"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let output_path = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("output_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                eprintln!("💻 生成代码到: {}", output_path);
                let result = self
                    .generate_code(specification, output_path)
                    .await;
                self.send_response(&request.id, result).await;
            }
            "review_code" => {
                let file_path = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("file_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                eprintln!("💻 评审代码: {}", file_path);
                let result = self.review_code(file_path).await;
                self.send_response(&request.id, result).await;
            }
            "ping" => {
                if let Some(ref client_arc) = self.client {
                    let mut client = client_arc.lock().await;
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": request.id,
                        "result": {
                            "success": true,
                            "status": "alive",
                            "agent_id": self.agent_id,
                        }
                    });
                    let _ = client
                        .send_raw(&serde_json::to_string(&resp).unwrap())
                        .await;
                }
            }
            "shutdown" => {
                eprintln!("💻 Code Agent 收到关闭信号");
                self.running = false;
            }
            other => {
                eprintln!("💻 未知方法: {}", other);
            }
        }
    }

    /// 发送 JSON-RPC 响应
    async fn send_response(&self, id: &str, result: serde_json::Value) {
        if let Some(ref client_arc) = self.client {
            let mut client = client_arc.lock().await;
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "success": true,
                    "result": result,
                }
            });
            let _ = client
                .send_raw(&serde_json::to_string(&resp).unwrap())
                .await;
        }
    }

    /// 读取文件内容
    async fn read_file(&self, file_path: &str) -> serde_json::Value {
        if file_path.is_empty() {
            return json!({
                "success": false,
                "error": "file_path 不能为空",
            });
        }

        let path = self.project_path.join(file_path);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let line_count = content.lines().count();
                let char_count = content.chars().count();
                json!({
                    "success": true,
                    "content": content,
                    "line_count": line_count,
                    "char_count": char_count,
                    "path": path.to_string_lossy().to_string(),
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("读取文件失败: {}", e),
                    "path": path.to_string_lossy().to_string(),
                })
            }
        }
    }

    /// 编辑文件（搜索替换 / 插入 / 删除 / 追加）
    async fn edit_file(
        &self,
        file_path: &str,
        operation: &str,
        search_text: &str,
        replace_text: &str,
        _insert_content: &str,
    ) -> serde_json::Value {
        if file_path.is_empty() {
            return json!({
                "success": false,
                "error": "file_path 不能为空",
            });
        }

        let path = self.project_path.join(file_path);

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let modified = match operation {
                    "search_replace" => {
                        if search_text.is_empty() {
                            return json!({
                                "success": false,
                                "error": "search_replace 操作需要 search 参数",
                            });
                        }
                        content.replace(search_text, replace_text)
                    }
                    _ => {
                        return json!({
                            "success": false,
                            "error": format!("不支持的操作: {}", operation),
                        });
                    }
                };

                // 检查是否有实际修改
                if modified == content {
                    return json!({
                        "success": false,
                        "error": "搜索文本未找到，文件未修改",
                        "search": search_text,
                    });
                }

                match tokio::fs::write(&path, &modified).await {
                    Ok(()) => {
                        let line_count = modified.lines().count();
                        json!({
                            "success": true,
                            "operation": operation,
                            "path": path.to_string_lossy().to_string(),
                            "line_count": line_count,
                            "modified": true,
                        })
                    }
                    Err(e) => {
                        json!({
                            "success": false,
                            "error": format!("写入文件失败: {}", e),
                        })
                    }
                }
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("读取文件失败: {}", e),
                })
            }
        }
    }

    /// 生成代码（写入指定路径）
    async fn generate_code(&self, specification: &str, output_path: &str) -> serde_json::Value {
        if specification.is_empty() {
            return json!({
                "success": false,
                "error": "specification 不能为空",
            });
        }

        if output_path.is_empty() {
            return json!({
                "success": false,
                "error": "output_path 不能为空",
            });
        }

        let path = self.project_path.join(output_path);

        // 确保父目录存在
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        match tokio::fs::write(&path, specification).await {
            Ok(()) => {
                json!({
                    "success": true,
                    "path": path.to_string_lossy().to_string(),
                    "bytes_written": specification.len(),
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("写入文件失败: {}", e),
                })
            }
        }
    }

    /// 评审代码（返回行数和基本统计）
    async fn review_code(&self, file_path: &str) -> serde_json::Value {
        if file_path.is_empty() {
            return json!({
                "success": false,
                "error": "file_path 不能为空",
            });
        }

        let path = self.project_path.join(file_path);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();
                let blank_lines = lines.iter().filter(|l| l.trim().is_empty()).count();
                let comment_lines = lines
                    .iter()
                    .filter(|l| {
                        let trimmed = l.trim();
                        trimmed.starts_with("//")
                            || trimmed.starts_with('#')
                            || trimmed.starts_with("/*")
                            || trimmed.starts_with('*')
                            || trimmed.starts_with("///")
                            || trimmed.starts_with("//!")
                    })
                    .count();
                let code_lines = total_lines - blank_lines - comment_lines;
                let extension = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default();

                json!({
                    "success": true,
                    "file_path": file_path,
                    "extension": extension,
                    "total_lines": total_lines,
                    "code_lines": code_lines,
                    "comment_lines": comment_lines,
                    "blank_lines": blank_lines,
                    "char_count": content.chars().count(),
                })
            }
            Err(e) => {
                json!({
                    "success": false,
                    "error": format!("读取文件失败: {}", e),
                })
            }
        }
    }

    /// 停止 Code Agent
    pub fn stop(&mut self) {
        self.running = false;
    }
}
