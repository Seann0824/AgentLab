// src/swarm/orchestrator.rs
// 🐝 Swarm Orchestrator — 蜂群编排器核心
//
// 封装 UDS Server + 子进程管理 + 消息路由
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::process::Child;
use tokio::sync::Mutex as TokioMutex;

use super::heartbeat::HeartbeatMonitor;
use super::registry::{AgentRegistration, AgentType, SwarmRegistry};
use super::rpc::{JsonRpcRequest, JsonRpcResponse};
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
    server: Option<UdsServer>,
    /// 蜂群注册表（共享引用用于心跳监控）
    registry: Arc<TokioMutex<SwarmRegistry>>,
    /// 已连接的 Agent 流（agent_id → UdsStream）
    streams: HashMap<String, Arc<TokioMutex<UdsStream>>>,
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

        crate::debug!("🐝 [Orchestrator] 蜂群编排器已启动 @ {:?}", socket);

        Ok(Self {
            server: Some(server),
            registry,
            streams: HashMap::new(),
            subprocesses: HashMap::new(),
            agent_types: HashMap::new(),
            _heartbeat_handle: Some(heartbeat_handle),
            socket_path: socket,
        })
    }

    /// 接受新的 Agent 连接（阻塞等待）
    pub async fn accept_agent(&mut self) -> Result<String> {
        let server = self
            .server
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("accept loop already owns the UDS server"))?;
        let (registration, stream) = server.accept().await?;
        let agent_id = registration.agent_id.clone();
        self.register_connected_agent(registration, stream).await;
        Ok(agent_id)
    }

    /// 启动后台连接接受循环
    pub fn start_accept_loop(orchestrator: Arc<TokioMutex<Self>>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut server = {
                let mut orch = orchestrator.lock().await;
                match orch.server.take() {
                    Some(server) => server,
                    None => {
                        crate::debug!("🐝 [Orchestrator] accept loop 已经启动，跳过重复启动");
                        return;
                    }
                }
            };

            loop {
                match server.accept().await {
                    Ok((registration, stream)) => {
                        let mut orch = orchestrator.lock().await;
                        orch.register_connected_agent(registration, stream).await;
                    }
                    Err(e) => {
                        crate::debug!("🐝 [Orchestrator] 接受连接失败: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        })
    }

    async fn register_connected_agent(
        &mut self,
        registration: AgentRegistration,
        stream: UdsStream,
    ) {
        let agent_id = registration.agent_id.clone();
        let agent_type = registration.agent_type.clone();
        {
            let mut reg = self.registry.lock().await;
            reg.register_agent(registration);
        }
        self.agent_types
            .insert(agent_id.clone(), agent_type.clone());
        self.streams
            .insert(agent_id.clone(), Arc::new(TokioMutex::new(stream)));
        crate::debug!(
            "🐝 [Orchestrator] Agent '{}' 已注册为 {}",
            agent_id,
            agent_type.as_str()
        );
    }

    /// 向指定 Agent 发送 JSON-RPC 请求
    pub async fn send_to_agent(&self, agent_id: &str, request: &JsonRpcRequest) -> Result<()> {
        let stream = self
            .streams
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not connected", agent_id))?;
        let mut stream = stream.lock().await;
        stream.send_request(request).await
    }

    /// 向指定 Agent 发送请求并等待响应（同步阻塞等待）
    ///
    /// 用于 dispatch_task 工具——派发任务给 Agent 后，
    /// 在同一连接上等待 Agent 返回执行结果。
    pub async fn send_request_and_wait(
        &self,
        agent_id: &str,
        request: &JsonRpcRequest,
        timeout_secs: u64,
    ) -> std::result::Result<JsonRpcResponse, String> {
        let stream = self
            .streams
            .get(agent_id)
            .cloned()
            .ok_or_else(|| format!("Agent '{}' not connected", agent_id))?;
        send_request_and_wait_on_stream(
            stream,
            self.registry.clone(),
            agent_id,
            request,
            timeout_secs,
        )
        .await
    }

    /// 在不持有 Orchestrator 全局锁的情况下发送请求并等待响应。
    pub async fn send_request_and_wait_shared(
        orchestrator: Arc<TokioMutex<Self>>,
        agent_id: &str,
        request: &JsonRpcRequest,
        timeout_secs: u64,
    ) -> std::result::Result<JsonRpcResponse, String> {
        let (stream, registry) = {
            let orch = orchestrator.lock().await;
            let stream = orch
                .streams
                .get(agent_id)
                .cloned()
                .ok_or_else(|| format!("Agent '{}' not connected", agent_id))?;
            (stream, orch.registry.clone())
        };
        send_request_and_wait_on_stream(stream, registry, agent_id, request, timeout_secs).await
    }

    /// 发送消息到所有 Agent（广播）
    pub async fn broadcast(
        &self,
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

    /// 根据 Agent 类型查找可用的 Agent ID
    ///
    /// 优先从 Registry 查找，回退到本地 streams 检测。
    pub async fn find_agent_by_type(&self, agent_type: &AgentType) -> Option<String> {
        // 1. 从 Registry 查找
        {
            let reg = self.registry.lock().await;
            let agents = reg.query_by_type(agent_type);
            for agent_info in &agents {
                let id = &agent_info.agent_id;
                if self.streams.contains_key(id) {
                    return Some(id.clone());
                }
            }
        }

        // 2. 回退：从本地 agent_types 映射查找（兼容旧注册方式）
        for (agent_id, atype) in &self.agent_types {
            if atype == agent_type && self.streams.contains_key(agent_id) {
                return Some(agent_id.clone());
            }
        }

        // 3. 如果完全找不到匹配类型的 Agent，返回第一个可用 stream
        let ids: Vec<String> = self.streams.keys().cloned().collect();
        ids.into_iter().next()
    }
}

async fn send_request_and_wait_on_stream(
    stream: Arc<TokioMutex<UdsStream>>,
    registry: Arc<TokioMutex<SwarmRegistry>>,
    agent_id: &str,
    request: &JsonRpcRequest,
    timeout_secs: u64,
) -> std::result::Result<JsonRpcResponse, String> {
    let timeout_secs = timeout_secs.max(1);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut stream = stream.lock().await;

    stream
        .send_request(request)
        .await
        .map_err(|e| format!("发送请求失败: {}", e))?;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "等待 Agent '{}' 响应超时 ({}s)",
                agent_id, timeout_secs
            ));
        }
        let remaining = deadline.saturating_duration_since(now);
        let value = match tokio::time::timeout(remaining, stream.read_json_value()).await {
            Ok(Ok(value)) => value,
            Ok(Err(e)) => return Err(format!("读取响应失败: {}", e)),
            Err(_) => {
                return Err(format!(
                    "等待 Agent '{}' 响应超时 ({}s)",
                    agent_id, timeout_secs
                ));
            }
        };

        if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
            handle_inbound_agent_request(&registry, agent_id, method, value.get("params")).await;
            continue;
        }

        let response: JsonRpcResponse =
            serde_json::from_value(value).map_err(|e| format!("解析响应失败: {}", e))?;
        if response.id == request.id {
            return Ok(response);
        }

        crate::debug!(
            "🐝 [Orchestrator] 忽略来自 Agent '{}' 的非匹配响应 id={} (等待 id={})",
            agent_id,
            response.id,
            request.id
        );
    }
}

async fn handle_inbound_agent_request(
    registry: &Arc<TokioMutex<SwarmRegistry>>,
    fallback_agent_id: &str,
    method: &str,
    params: Option<&serde_json::Value>,
) {
    match method {
        "heartbeat" => {
            let agent_id = params
                .and_then(|p| p.get("agent_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(fallback_agent_id);
            let mut reg = registry.lock().await;
            if !reg.heartbeat(agent_id) {
                crate::debug!("🐝 [Orchestrator] 收到未知 Agent '{}' 的心跳", agent_id);
            }
        }
        "unregister" => {
            let agent_id = params
                .and_then(|p| p.get("agent_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(fallback_agent_id);
            let mut reg = registry.lock().await;
            reg.unregister(agent_id);
        }
        other => {
            crate::debug!(
                "🐝 [Orchestrator] 等待任务响应时收到 Agent '{}' 的请求 '{}'，已忽略",
                fallback_agent_id,
                other
            );
        }
    }
}

impl Drop for SwarmOrchestrator {
    fn drop(&mut self) {
        // 清理 socket 文件
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
