// src/swarm/registry.rs
// Agent 注册与发现 — SwarmRegistry 数据结构
//
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

// ===================================================================
// 基本类型
// ===================================================================

/// Agent 类型
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentType {
    #[serde(rename = "orchestrator")]
    Orchestrator,
    #[serde(rename = "memory")]
    Memory,
    #[serde(rename = "general")]
    General,
    #[serde(rename = "verifier")]
    Verifier,
    #[serde(rename = "coder")]
    Coder,
    #[serde(rename = "researcher")]
    Researcher,
    #[serde(rename = "reader")]
    Reader,
    #[serde(rename = "custom")]
    Custom(String),
}

impl AgentType {
    pub fn as_str(&self) -> &str {
        match self {
            AgentType::Orchestrator => "orchestrator",
            AgentType::Memory => "memory",
            AgentType::General => "general",
            AgentType::Verifier => "verifier",
            AgentType::Coder => "coder",
            AgentType::Researcher => "researcher",
            AgentType::Reader => "reader",
            AgentType::Custom(s) => s.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "orchestrator" => AgentType::Orchestrator,
            "memory" => AgentType::Memory,
            "general" => AgentType::General,
            "verifier" => AgentType::Verifier,
            "coder" => AgentType::Coder,
            "researcher" => AgentType::Researcher,
            "reader" => AgentType::Reader,
            _ => AgentType::Custom(s.to_string()),
        }
    }
}

/// Agent 状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    #[serde(rename = "online")]
    Online,
    #[serde(rename = "busy")]
    Busy,
    #[serde(rename = "offline")]
    Offline,
    #[serde(rename = "failed")]
    Failed,
}

impl AgentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            AgentStatus::Online => "online",
            AgentStatus::Busy => "busy",
            AgentStatus::Offline => "offline",
            AgentStatus::Failed => "failed",
        }
    }
}

/// Agent 注册信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Agent 唯一 ID
    pub agent_id: String,
    /// Agent 类型
    pub agent_type: AgentType,
    /// Agent 状态
    pub status: AgentStatus,
    /// Agent 主机名
    pub hostname: String,
    /// 连接时间戳
    pub connected_at: u64,
    /// 最后心跳时间戳
    pub last_heartbeat: u64,
    /// Agent 元数据（可选扩展）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ===================================================================
// SwarmRegistry
// ===================================================================

/// 蜂群注册表 — 管理所有 Agent 的注册信息
#[derive(Debug, Clone)]
pub struct SwarmRegistry {
    /// agent_id → AgentInfo
    agents: HashMap<String, AgentInfo>,
    /// 心跳超时阈值
    heartbeat_timeout: Duration,
}

impl SwarmRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            heartbeat_timeout: Duration::from_secs(30),
        }
    }

    /// 设置心跳超时阈值
    pub fn with_heartbeat_timeout(mut self, timeout: Duration) -> Self {
        self.heartbeat_timeout = timeout;
        self
    }

    /// 注册 Agent
    pub fn register(&mut self, agent_id: String, agent_type: AgentType) -> AgentInfo {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let info = AgentInfo {
            agent_id: agent_id.clone(),
            agent_type,
            status: AgentStatus::Online,
            hostname: hostname(),
            connected_at: now,
            last_heartbeat: now,
            metadata: None,
        };

        self.agents.insert(agent_id, info.clone());
        info
    }

    /// 注销 Agent
    pub fn unregister(&mut self, agent_id: &str) -> Option<AgentInfo> {
        self.agents.remove(agent_id)
    }

    /// 更新心跳
    pub fn heartbeat(&mut self, agent_id: &str) -> bool {
        if let Some(info) = self.agents.get_mut(agent_id) {
            let now = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            info.last_heartbeat = now;
            info.status = AgentStatus::Online;
            true
        } else {
            false
        }
    }

    /// 设置 Agent 状态
    pub fn set_status(&mut self, agent_id: &str, status: AgentStatus) -> bool {
        if let Some(info) = self.agents.get_mut(agent_id) {
            info.status = status;
            true
        } else {
            false
        }
    }

    /// 获取指定 Agent 信息
    pub fn get(&self, agent_id: &str) -> Option<&AgentInfo> {
        self.agents.get(agent_id)
    }

    /// 获取所有 Agent
    pub fn all_agents(&self) -> Vec<&AgentInfo> {
        self.agents.values().collect()
    }

    /// 按类型查询 Agent
    pub fn query_by_type(&self, agent_type: &AgentType) -> Vec<&AgentInfo> {
        self.agents
            .values()
            .filter(|a| a.agent_type == *agent_type)
            .collect()
    }

    /// 按状态查询 Agent
    pub fn query_by_status(&self, status: &AgentStatus) -> Vec<&AgentInfo> {
        self.agents
            .values()
            .filter(|a| a.status == *status)
            .collect()
    }

    /// 获取在线 Agent 数量
    pub fn online_count(&self) -> usize {
        self.query_by_status(&AgentStatus::Online).len()
    }

    /// 检查是否有心跳超时的 Agent
    pub fn check_timeouts(&mut self) -> Vec<String> {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut timed_out = Vec::new();

        for (id, info) in &self.agents {
            let elapsed = now - info.last_heartbeat;
            if Duration::from_secs(elapsed) > self.heartbeat_timeout {
                timed_out.push(id.clone());
            }
        }

        for id in &timed_out {
            if let Some(info) = self.agents.get_mut(id) {
                info.status = AgentStatus::Offline;
            }
        }

        timed_out
    }

    /// 将注册表导出为 JSON 格式
    pub fn to_json(&self) -> serde_json::Value {
        let agents: Vec<&AgentInfo> = self.agents.values().collect();
        serde_json::json!({
            "agents": agents,
            "online_count": self.online_count(),
            "total_count": self.agents.len(),
        })
    }
}

impl Default for SwarmRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 获取主机名
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "localhost".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_query() {
        let mut registry = SwarmRegistry::new();

        registry.register("agent-1".to_string(), AgentType::Memory);
        registry.register("agent-2".to_string(), AgentType::General);

        assert_eq!(registry.all_agents().len(), 2);
        assert_eq!(registry.online_count(), 2);

        let memory_agents = registry.query_by_type(&AgentType::Memory);
        assert_eq!(memory_agents.len(), 1);
        assert_eq!(memory_agents[0].agent_id, "agent-1");
    }

    #[test]
    fn test_heartbeat_and_timeout() {
        let mut registry = SwarmRegistry::new();
        registry.register("agent-1".to_string(), AgentType::Memory);

        assert!(registry.heartbeat("agent-1"));
        assert!(!registry.heartbeat("non-existent"));
    }

    #[test]
    fn test_unregister() {
        let mut registry = SwarmRegistry::new();
        registry.register("agent-1".to_string(), AgentType::Memory);

        let removed = registry.unregister("agent-1");
        assert!(removed.is_some());
        assert!(registry.get("agent-1").is_none());
    }

    #[test]
    fn test_set_status() {
        let mut registry = SwarmRegistry::new();
        registry.register("agent-1".to_string(), AgentType::Memory);

        assert!(registry.set_status("agent-1", AgentStatus::Busy));
        assert_eq!(registry.get("agent-1").unwrap().status, AgentStatus::Busy);

        assert!(!registry.set_status("non-existent", AgentStatus::Busy));
    }
}
