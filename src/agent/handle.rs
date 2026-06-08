/// AgentHandle — 多 Agent 运行的句柄，通过 tokio::task::spawn 管理
pub struct AgentHandle {
    /// Agent 名称
    pub name: String,
    /// tokio 任务句柄
    pub task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl AgentHandle {
    /// 获取 Agent 名称
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 等待 Agent 完成
    pub async fn join(self) -> anyhow::Result<()> {
        self.task
            .await
            .map_err(|e| anyhow::anyhow!("Agent '{}' panicked: {}", self.name, e))?
    }
}
