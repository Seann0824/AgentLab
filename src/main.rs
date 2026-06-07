use anyhow;

use agent_lab::{
    agent::Agent,
    model::ModelManager,
};

/// Agent Lab — 自我进化的 AI Agent 框架
///
/// main.rs 现在是一个薄壳，只负责：
/// 1. 使用 ModelManager::from_env() 从环境变量加载模型配置
/// 2. 通过 AgentBuilder 构建 Agent
/// 3. 运行 Agent 主循环

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let model_manager = ModelManager::from_env();
    if !model_manager.has_models() {
        anyhow::bail!("未从环境变量发现任何模型配置。请设置 <PREFIX>_API_KEY 和 <PREFIX>_BASE_URL 环境变量。");
    }

    let mut agent = Agent::builder()
        .model_manager(model_manager)
        .build()?;

    agent.run().await
}
