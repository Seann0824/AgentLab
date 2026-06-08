// src/swarm/agents/verifier.rs
// ✅ Verifier Agent — 代码验证 Agent
//
// Verifier Agent 是一个非交互式 Agent，通过 UDS 与 Orchestrator 通信。
// 职责：
// 1. 代码验证 — 运行 cargo check / cargo test
// 2. 回归测试 — 运行预定义的测试套件
// 3. 编译错误分析 — 解析编译输出并返回结构化结果

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use tokio::sync::Mutex as TokioMutex;
use tokio::time::interval;

use crate::swarm::agents::common::{send_task_result, task_failed, task_success};
use crate::swarm::heartbeat::create_heartbeat_request;
use crate::swarm::registry::AgentType;
use crate::swarm::rpc::JsonRpcRequest;
use crate::swarm::task::SwarmTask;
use crate::swarm::transport::{UdsClient, default_socket_path};

/// Verifier Agent — 代码验证 Agent
pub struct VerifierAgent {
    /// Agent ID
    agent_id: String,
    /// UDS 客户端（连接到 Orchestrator），用 Arc<Mutex> 共享给心跳任务
    client: Option<Arc<TokioMutex<UdsClient>>>,
    /// 是否正在运行
    running: bool,
    /// 项目路径
    project_path: PathBuf,
}

impl VerifierAgent {
    /// 创建新的 Verifier Agent
    pub fn new(project_path: Option<PathBuf>) -> Self {
        Self {
            agent_id: format!("verifier-{}", std::process::id()),
            client: None,
            running: false,
            project_path: project_path.unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    /// 连接到 Orchestrator
    pub async fn connect(&mut self, orchestrator_socket: Option<PathBuf>) -> Result<()> {
        let socket = orchestrator_socket.unwrap_or_else(default_socket_path);
        eprintln!("✅ Verifier Agent 连接到 Orchestrator @ {:?}", socket);

        let client = UdsClient::connect_as(&socket, &self.agent_id, AgentType::Verifier)
            .await
            .context(format!("无法连接到 Orchestrator (socket: {:?})", socket))?;

        eprintln!("✅ Verifier Agent '{}' 已注册到蜂群", self.agent_id);

        self.client = Some(Arc::new(TokioMutex::new(client)));
        Ok(())
    }

    /// 运行 Verifier Agent 主循环
    pub async fn run(&mut self) -> Result<()> {
        self.running = true;
        eprintln!("✅ Verifier Agent 主循环已启动");

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
                        eprintln!("✅ [Heartbeat] 发送失败: {}", e);
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
                        eprintln!("✅ 读取请求失败: {}", e);
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
            "dispatch_task" => {
                let task_result = match SwarmTask::from_rpc_params(request.params.as_ref()) {
                    Ok(task) => {
                        let desc = task.description();
                        let params = task.params();
                        let lower = desc.to_lowercase();
                        let result =
                            if lower.contains("test") || params.get("test_filter").is_some() {
                                let test_filter = params
                                    .get("test_filter")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                eprintln!("✅ 结构化任务执行 cargo test (filter: {})", test_filter);
                                self.run_cargo_test(test_filter).await
                            } else {
                                eprintln!("✅ 结构化任务执行 cargo check");
                                self.run_cargo_check().await
                            };
                        task_success(&task, result)
                    }
                    Err(err) => task_failed("unknown", err),
                };
                send_task_result(&self.client, &request.id, task_result).await;
            }
            "run_cargo_check" => {
                eprintln!("✅ 执行 cargo check...");
                let result = self.run_cargo_check().await;
                self.send_response(&request.id, result).await;
            }
            "run_cargo_test" => {
                let test_filter = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("test_filter"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                eprintln!("✅ 执行 cargo test (filter: {})...", test_filter);
                let result = self.run_cargo_test(test_filter).await;
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
                eprintln!("✅ Verifier Agent 收到关闭信号");
                self.running = false;
            }
            other => {
                eprintln!("✅ 未知方法: {}", other);
            }
        }
    }

    /// 运行 cargo check
    async fn run_cargo_check(&self) -> serde_json::Value {
        let start = std::time::Instant::now();

        match tokio::process::Command::new("cargo")
            .args(["check"])
            .current_dir(&self.project_path)
            .output()
            .await
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let elapsed = start.elapsed();

                let passed = output.status.success();
                let mut errors = Vec::new();
                let mut warnings = Vec::new();

                // 简单解析输出中的错误和警告
                for line in stderr.lines() {
                    if line.contains("error[") || line.contains("error:") {
                        errors.push(line.to_string());
                    } else if line.contains("warning[") || line.contains("warning:") {
                        warnings.push(line.to_string());
                    }
                }

                json!({
                    "passed": passed,
                    "exit_code": output.status.code().unwrap_or(-1),
                    "duration_ms": elapsed.as_millis(),
                    "stdout": stdout,
                    "stderr": stderr,
                    "errors": errors,
                    "warnings": warnings,
                    "error_count": errors.len(),
                    "warning_count": warnings.len(),
                })
            }
            Err(e) => {
                json!({
                    "passed": false,
                    "error": format!("执行 cargo check 失败: {}", e),
                    "exit_code": -1,
                })
            }
        }
    }

    /// 运行 cargo test
    async fn run_cargo_test(&self, test_filter: &str) -> serde_json::Value {
        let start = std::time::Instant::now();

        let mut cmd = tokio::process::Command::new("cargo");
        cmd.arg("test").current_dir(&self.project_path);

        if !test_filter.is_empty() {
            cmd.arg("--").arg(test_filter);
        }

        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let elapsed = start.elapsed();

                let passed = output.status.success();

                json!({
                    "passed": passed,
                    "exit_code": output.status.code().unwrap_or(-1),
                    "duration_ms": elapsed.as_millis(),
                    "stdout": stdout,
                    "stderr": stderr,
                })
            }
            Err(e) => {
                json!({
                    "passed": false,
                    "error": format!("执行 cargo test 失败: {}", e),
                    "exit_code": -1,
                })
            }
        }
    }

    /// 发送响应
    async fn send_response(&self, request_id: &str, result: serde_json::Value) {
        if let Some(ref client_arc) = self.client {
            let mut client = client_arc.lock().await;
            let resp = json!({
                "jsonrpc": "2.0",
                "id": request_id,
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

    /// 停止 Verifier Agent
    pub fn stop(&mut self) {
        self.running = false;
    }
}
