// src/swarm/pool.rs
// 🗄️ Agent Pool — 可复用的 Agent 实例池
//
// 管理多个同类型 Agent 实例的池化复用。
// 用于 General Agent 池和 Verifier Agent 池，
// 支持按需创建、回收、伸缩。

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use std::fmt;

use anyhow::Result;
use tokio::sync::Mutex as TokioMutex;

use super::transport::{UdsClient, default_socket_path};

/// Agent 实例状态
#[derive(Debug, Clone, PartialEq)]
pub enum AgentInstanceStatus {
    /// 空闲（可分配）
    Idle,
    /// 忙碌（正在处理任务）
    Busy,
    /// 异常（不可用）
    Error,
}

/// Agent 实例
pub struct AgentInstance {
    /// Agent ID
    pub id: String,
    /// 类型
    pub agent_type: PoolAgentType,
    /// 状态
    pub status: AgentInstanceStatus,
    /// 创建时间
    pub created_at: Instant,
    /// 最后使用时间
    pub last_used: Instant,
    /// UDS 客户端（连接到该 Agent）
    pub client: Option<Arc<TokioMutex<UdsClient>>>,
}

/// 池中 Agent 类型

impl fmt::Debug for AgentInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentInstance")
            .field("id", &self.id)
            .field("agent_type", &self.agent_type)
            .field("status", &self.status)
            .field("created_at", &self.created_at)
            .field("last_used", &self.last_used)
            .field("client", &self.client.as_ref().map(|_| "UdsClient(...)"))
            .finish()
    }
}

impl fmt::Debug for AgentPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentPool")
            .field("name", &self.name)
            .field("agent_type", &self.agent_type)
            .field("idle_count", &self.idle.len())
            .field("busy_count", &self.busy.len())
            .field("min_size", &self.min_size)
            .field("max_size", &self.max_size)
            .field("idle_timeout", &self.idle_timeout)
            .field("orchestrator_socket", &self.orchestrator_socket)
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum PoolAgentType {
    /// General Agent（通用任务执行）
    General,
    /// Verifier Agent（代码验证）
    Verifier,
    /// 自定义类型
    Custom(String),
}

/// Agent Pool — 可复用实例池
pub struct AgentPool {
    /// 池名称
    name: String,
    /// 池中类型
    agent_type: PoolAgentType,
    /// 可用实例队列
    idle: VecDeque<AgentInstance>,
    /// 忙碌实例列表
    busy: Vec<AgentInstance>,
    /// 最小池大小
    min_size: usize,
    /// 最大池大小
    max_size: usize,
    /// 空闲超时回收时长
    idle_timeout: Duration,
    /// Orchestrator 的 UDS socket 路径
    orchestrator_socket: std::path::PathBuf,
}

impl AgentPool {
    /// 创建新的 Agent Pool
    pub fn new(
        name: String,
        agent_type: PoolAgentType,
        min_size: usize,
        max_size: usize,
        orchestrator_socket: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            name,
            agent_type,
            idle: VecDeque::new(),
            busy: Vec::new(),
            min_size,
            max_size,
            idle_timeout: Duration::from_secs(300), // 5 分钟
            orchestrator_socket: orchestrator_socket.unwrap_or_else(default_socket_path),
        }
    }

    /// 初始化池（创建 min_size 个实例）
    pub async fn initialize(&mut self) -> Result<()> {
        for i in 0..self.min_size {
            let instance = self.spawn_instance(i).await?;
            self.idle.push_back(instance);
        }
        eprintln!(
            "🗄️ AgentPool '{}' 初始化完成: {} 个实例",
            self.name, self.min_size
        );
        Ok(())
    }

    /// 生成一个 Agent 实例（子进程 + UDS 连接）
    async fn spawn_instance(&self, index: usize) -> Result<AgentInstance> {
        let agent_type_str = match &self.agent_type {
            PoolAgentType::General => "general",
            PoolAgentType::Verifier => "verifier",
            PoolAgentType::Custom(t) => t,
        };

        let instance_id = format!("{}-{}-{}", self.name, agent_type_str, index);

        // 启动子 Agent 进程
        let binary = std::env::current_exe().ok();
        let binary_path = binary
            .as_deref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "agent-lab".to_string());
        let socket_str = self.orchestrator_socket.to_string_lossy().to_string();

        match std::process::Command::new(&binary_path)
            .arg("--agent-type")
            .arg(agent_type_str)
            .arg("--orchestrator-socket")
            .arg(&socket_str)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                eprintln!("🗄️ 生成 Agent 实例 '{}' (PID: {})", instance_id, child.id());
                // 不等待——子进程独立运行
                std::mem::drop(child);

                Ok(AgentInstance {
                    id: instance_id,
                    agent_type: self.agent_type.clone(),
                    status: AgentInstanceStatus::Idle,
                    created_at: Instant::now(),
                    last_used: Instant::now(),
                    client: None,
                })
            }
            Err(e) => {
                anyhow::bail!("生成 Agent 实例 '{}' 失败: {}", instance_id, e);
            }
        }
    }

    /// 从池中获取一个空闲实例
    pub async fn acquire(&mut self) -> Option<&mut AgentInstance> {
        // 回收超时空闲实例
        self.recycle_idle().await;

        if let Some(mut instance) = self.idle.pop_front() {
            instance.status = AgentInstanceStatus::Busy;
            instance.last_used = Instant::now();
            let id = instance.id.clone();
            self.busy.push(instance);
            let idx = self.busy.len() - 1;
            eprintln!(
                "🗄️ 分配实例 '{}' (池: {}, 忙碌: {})",
                id,
                self.name,
                self.busy.len()
            );
            Some(&mut self.busy[idx])
        } else if (self.idle.len() + self.busy.len()) < self.max_size {
            // 池未满，创建新实例
            let idx = self.idle.len() + self.busy.len();
            match self.spawn_instance(idx).await {
                Ok(mut instance) => {
                    instance.status = AgentInstanceStatus::Busy;
                    instance.last_used = Instant::now();
                    let id = instance.id.clone();
                    self.busy.push(instance);
                    let idx = self.busy.len() - 1;
                    eprintln!(
                        "🗄️ 创建并分配实例 '{}' (池: {}, 忙碌: {})",
                        id,
                        self.name,
                        self.busy.len()
                    );
                    Some(&mut self.busy[idx])
                }
                Err(e) => {
                    eprintln!("🗄️ 创建实例失败: {}", e);
                    None
                }
            }
        } else {
            eprintln!(
                "🗄️ 池已满('{}', 最大: {}), 无可用实例",
                self.name, self.max_size
            );
            None
        }
    }

    /// 归还实例到空闲池
    pub async fn release(&mut self, instance_id: &str) -> Result<()> {
        if let Some(pos) = self.busy.iter().position(|i| i.id == instance_id) {
            let mut instance = self.busy.remove(pos);
            instance.status = AgentInstanceStatus::Idle;
            instance.last_used = Instant::now();
            self.idle.push_back(instance);
            eprintln!(
                "🗄️ 回收实例 '{}' (池: {}, 空闲: {})",
                instance_id,
                self.name,
                self.idle.len()
            );
            Ok(())
        } else {
            anyhow::bail!("未找到忙碌实例 '{}'", instance_id);
        }
    }

    /// 回收超时的空闲实例
    async fn recycle_idle(&mut self) {
        let now = Instant::now();
        let timeout = self.idle_timeout;
        let min = self.min_size;

        let before = self.idle.len();
        // 只在空闲数量超过最小池大小时才回收
        if before <= min {
            return;
        }
        self.idle
            .retain(|instance| now.duration_since(instance.last_used) < timeout);
        let recycled = before - self.idle.len();
        if recycled > 0 {
            eprintln!(
                "🗄️ 回收了 {} 个超时空闲实例 (池: {}, 空闲: {})",
                recycled,
                self.name,
                self.idle.len()
            );
        }
    }

    /// 获取池统计信息
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            name: self.name.clone(),
            agent_type: format!("{:?}", self.agent_type),
            idle: self.idle.len(),
            busy: self.busy.len(),
            total: self.idle.len() + self.busy.len(),
            min_size: self.min_size,
            max_size: self.max_size,
        }
    }

    /// 池大小
    pub fn size(&self) -> usize {
        self.idle.len() + self.busy.len()
    }

    /// 检查是否有空闲实例
    pub fn has_idle(&self) -> bool {
        !self.idle.is_empty()
    }
}

/// 池统计信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct PoolStats {
    pub name: String,
    pub agent_type: String,
    pub idle: usize,
    pub busy: usize,
    pub total: usize,
    pub min_size: usize,
    pub max_size: usize,
}

/// AgentPoolManager — 管理多个不同类型的 Agent Pool
#[derive(Debug)]
pub struct AgentPoolManager {
    /// General Agent 池
    pub general_pool: AgentPool,
    /// Verifier Agent 池
    pub verifier_pool: AgentPool,
    /// 自定义池
    custom_pools: Vec<AgentPool>,
}

impl AgentPoolManager {
    /// 创建新的 AgentPoolManager
    pub fn new(orchestrator_socket: Option<std::path::PathBuf>) -> Self {
        Self {
            general_pool: AgentPool::new(
                "general".to_string(),
                PoolAgentType::General,
                1, // min 1
                5, // max 5
                orchestrator_socket.clone(),
            ),
            verifier_pool: AgentPool::new(
                "verifier".to_string(),
                PoolAgentType::Verifier,
                0, // min 0 (按需创建)
                3, // max 3
                orchestrator_socket,
            ),
            custom_pools: Vec::new(),
        }
    }

    /// 初始化所有池
    pub async fn initialize_all(&mut self) -> Result<()> {
        eprintln!("🗄️ 初始化所有 Agent 池...");
        self.general_pool.initialize().await?;
        self.verifier_pool.initialize().await?;
        for pool in &mut self.custom_pools {
            pool.initialize().await?;
        }
        eprintln!("🗄️ 所有 Agent 池初始化完成");
        Ok(())
    }

    /// 获取所有池的统计信息
    pub fn all_stats(&self) -> Vec<PoolStats> {
        let mut stats = vec![self.general_pool.stats(), self.verifier_pool.stats()];
        for pool in &self.custom_pools {
            stats.push(pool.stats());
        }
        stats
    }

    /// 添加自定义池
    pub fn add_pool(&mut self, pool: AgentPool) {
        self.custom_pools.push(pool);
    }
}
