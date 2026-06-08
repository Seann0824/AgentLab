// src/swarm/heartbeat.rs
// 心跳检测机制 — Agent 健康监控
//
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;

use super::registry::SwarmRegistry;
use super::rpc::JsonRpcRequest;

/// 心跳检测器 — 定期检查 Agent 心跳超时
pub struct HeartbeatMonitor {
    /// 蜂群注册表
    registry: Arc<Mutex<SwarmRegistry>>,
    /// 检查间隔
    check_interval: Duration,
}

impl HeartbeatMonitor {
    /// 创建心跳检测器
    pub fn new(registry: Arc<Mutex<SwarmRegistry>>) -> Self {
        Self {
            registry,
            check_interval: Duration::from_secs(10),
        }
    }

    /// 设置检查间隔
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }

    /// 启动心跳监控任务（后台运行）
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = interval(self.check_interval);
            loop {
                ticker.tick().await;
                let mut registry = self.registry.lock().await;
                let timed_out = registry.check_timeouts();
                if !timed_out.is_empty() {
                    eprintln!("🐝 [Heartbeat] Agents timed out: {:?}", timed_out);
                }
            }
        })
    }
}

/// 创建一个心跳消息（用于 Agent 定时发送）
pub fn create_heartbeat_request(agent_id: &str) -> JsonRpcRequest {
    JsonRpcRequest::new(
        "heartbeat",
        Some(serde_json::json!({
            "agent_id": agent_id,
        })),
    )
}
