// src/swarm/agents/general.rs
// 🔧 General Agent — 通用任务执行 Agent
//
// General Agent 是一个非交互式 Agent，通过 UDS 与 Orchestrator 通信。
// 职责：
// 1. 接收并执行 Orchestrator 派发的通用任务
// 2. 文件读取、代码搜索、代码修改
// 3. 工具调用的独立执行环境
// 4. 结果返回给 Orchestrator

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

/// General Agent — 通用任务执行 Agent
pub struct GeneralAgent {
    /// Agent ID
    agent_id: String,
    /// UDS 客户端（连接到 Orchestrator），用 Arc<Mutex> 共享给心跳任务
    client: Option<Arc<TokioMutex<UdsClient>>>,
    /// 是否正在运行
    running: bool,
    /// 当前任务描述
    current_task: Option<String>,
}

impl GeneralAgent {
    /// 创建新的 General Agent
    pub fn new() -> Self {
        Self {
            agent_id: format!("general-{}", std::process::id()),
            client: None,
            running: false,
            current_task: None,
        }
    }

    /// 连接到 Orchestrator
    pub async fn connect(&mut self, orchestrator_socket: Option<PathBuf>) -> Result<()> {
        let socket = orchestrator_socket.unwrap_or_else(default_socket_path);
        eprintln!("🔧 General Agent 连接到 Orchestrator @ {:?}", socket);

        let client = UdsClient::connect(&socket, &self.agent_id)
            .await
            .context(format!("无法连接到 Orchestrator (socket: {:?})", socket))?;

        eprintln!("🔧 General Agent '{}' 已注册到蜂群", self.agent_id);

        self.client = Some(Arc::new(TokioMutex::new(client)));
        Ok(())
    }

    /// 运行 General Agent 主循环
    pub async fn run(&mut self) -> Result<()> {
        self.running = true;
        eprintln!("🔧 General Agent 主循环已启动");

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
                        eprintln!("🔧 [Heartbeat] 发送失败: {}", e);
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
                        eprintln!("🔧 读取请求失败: {}", e);
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
            "execute_task" => {
                let task = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("task"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let params = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("params"))
                    .cloned();

                eprintln!("🔧 收到任务: {}", &task[..task.len().min(100)]);
                self.current_task = Some(task.clone());

                // 执行任务（目前返回接收确认，实际执行由 Orchestrator 协调）
                let result = json!({
                    "status": "received",
                    "task": task,
                    "params": params,
                    "message": format!("General Agent '{}' 已接收任务", self.agent_id),
                });

                if let Some(ref client_arc) = self.client {
                    let mut client = client_arc.lock().await;
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": request.id,
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
                eprintln!("🔧 General Agent 收到关闭信号");
                self.running = false;
            }
            other => {
                eprintln!("🔧 未知方法: {}", other);
            }
        }
    }

    /// 停止 General Agent
    pub fn stop(&mut self) {
        self.running = false;
    }
}

impl Default for GeneralAgent {
    fn default() -> Self {
        Self::new()
    }
}
