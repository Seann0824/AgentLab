// src/swarm/orchestrator.rs
// 🐝 Swarm Orchestrator — 蜂群编排器核心
//
// 封装 UDS Server + 子进程管理 + 消息路由
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::process::Child;
use tokio::sync::Mutex as TokioMutex;

use super::heartbeat::HeartbeatMonitor;
use super::registry::{AgentType, SwarmRegistry};
use super::rpc::JsonRpcRequest;
use super::transport::{UdsServer, UdsStream, default_socket_path};

/// 已连接的 Agent 信息（包含活动流）
pub struct ConnectedAgent {
    /// Agent ID
    pub agent_id: String,
    /// 活动 UDS 流（用于双向通信）
    pub stream: UdsStream,
}

/// Swarm Orchestrator — 蜂群编排器
pub struct SwarmOrchestrator {
    /// UDS 服务器
    server: UdsServer,
    /// 蜂群注册表（共享引用用于心跳监控）
    registry: Arc<TokioMutex<SwarmRegistry>>,
    /// 已连接的 Agent 流（agent_id → UdsStream）
    streams: HashMap<String, UdsStream>,
    /// 已启动的子进程（agent_id → Child）
    #[allow(dead_code)]
    subprocesses: HashMap<String, Child>,
    /// Agent 类型映射（agent_id → AgentType）
    agent_types: HashMap<String, AgentType>,
    /// 心跳监控任务句柄
    _heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
    /// Socket 路径
    socket_path: PathBuf,
}

impl SwarmOrchestrator {
    /// 创建并绑定 Swarm Orchestrator
    pub async fn bind(socket_path: Option<PathBuf>) -> Result<Self> {
        let socket = socket_path.unwrap_or_else(default_socket_path);
        let server = UdsServer::bind(&socket).await?;

        let registry = Arc::new(TokioMutex::new(SwarmRegistry::new()));

        // 注册自己（Orchestrator）
        {
            let mut reg = registry.lock().await;
            reg.register("orchestrator-1".to_string(), AgentType::Orchestrator);
        }

        // 启动心跳监控
        let monitor = HeartbeatMonitor::new(registry.clone());
        let heartbeat_handle = monitor.start();

        eprintln!("🐝 [Orchestrator] 蜂群编排器已启动 @ {:?}", socket);

        Ok(Self {
            server,
            registry,
            streams: HashMap::new(),
            subprocesses: HashMap::new(),
            agent_types: HashMap::new(),
            _heartbeat_handle: Some(heartbeat_handle),
            socket_path: socket,
        })
    }

    /// 接受新的 Agent 连接（阻塞等待）
    pub async fn accept_agent(&mut self) -> Result<(String, &mut UdsStream)> {
        let (agent_id, stream) = self.server.accept().await?;

        // 注册到 SwarmRegistry
        {
            let mut reg = self.registry.lock().await;
            reg.register(agent_id.clone(), AgentType::Memory);
        }

        // 存储流引用
        self.streams.insert(agent_id.clone(), stream);

        eprintln!("🐝 [Orchestrator] Agent '{}' 已注册", agent_id);

        // 这里需要返回可变引用，但我们用 HashMap 存储，用 get_mut
        let stream_ref = self
            .streams
            .get_mut(&agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found after insert", agent_id))?;

        Ok((agent_id, stream_ref))
    }

    /// 启动后台连接接受循环
    pub fn start_accept_loop(orchestrator: Arc<TokioMutex<Self>>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let mut orch = orchestrator.lock().await;
                match orch.server.accept().await {
                    Ok((agent_id, stream)) => {
                        // 注册到 SwarmRegistry
                        {
                            let mut reg = orch.registry.lock().await;
                            reg.register(agent_id.clone(), AgentType::Memory);
                        }
                        eprintln!("🐝 [Orchestrator] Agent '{}' 已连接", agent_id);
                        orch.streams.insert(agent_id, stream);
                    }
                    Err(e) => {
                        eprintln!("🐝 [Orchestrator] 接受连接失败: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        })
    }

    /// 向指定 Agent 发送 JSON-RPC 请求
    pub async fn send_to_agent(&mut self, agent_id: &str, request: &JsonRpcRequest) -> Result<()> {
        let stream = self
            .streams
            .get_mut(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not connected", agent_id))?;
        stream.send_request(request).await
    }

    /// 发送消息到所有 Agent（广播）
    pub async fn broadcast(
        &mut self,
        request: &super::rpc::JsonRpcRequest,
    ) -> Vec<(String, Result<()>)> {
        let mut results = Vec::new();
        let agent_ids: Vec<String> = self.streams.keys().cloned().collect();
        for agent_id in agent_ids {
            let result = self.send_to_agent(&agent_id, request).await;
            results.push((agent_id, result));
        }
        results
    }

    /// 获取已连接的 Agent 列表
    pub fn connected_agents(&self) -> Vec<&str> {
        self.streams.keys().map(|s| s.as_str()).collect()
    }

    /// 获取 Agent 数量
    pub fn agent_count(&self) -> usize {
        self.streams.len()
    }

    /// 注册 Agent 类型
    pub fn set_agent_type(&mut self, agent_id: String, agent_type: AgentType) {
        self.agent_types.insert(agent_id, agent_type);
    }

    /// 获取 Registry 的共享引用
    pub fn registry(&self) -> Arc<TokioMutex<SwarmRegistry>> {
        self.registry.clone()
    }

    /// 获取注册表快照
    pub async fn get_registry_snapshot(&self) -> SwarmRegistry {
        self.registry.lock().await.clone()
    }

    /// 获取已注册的类型列表
    pub fn agent_type_summary(&self) -> HashMap<String, String> {
        self.agent_types
            .iter()
            .map(|(id, t)| (id.clone(), format!("{:?}", t)))
            .collect()
    }

    /// 生成 Agent ID（带前缀和后缀）
    pub fn generate_agent_id(prefix: &str) -> String {
        format!("{}-{}", prefix, std::process::id())
    }
}

impl Drop for SwarmOrchestrator {
    fn drop(&mut self) {
        // 清理 socket 文件
        let path = self.socket_path.clone();
        tokio::task::block_in_place(|| {
            std::fs::remove_file(&path).ok();
        });
    }
}
