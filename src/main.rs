use std::env;
use anyhow;
use dotenvy;

use agent_lab::{
    agent::Agent,
    model::{ModelAdapter, OpenAiCompatibleAdapter},
};

/// Agent Lab — 自我进化的 AI Agent 框架
///
/// main.rs 现在是一个薄壳，只负责：
/// 1. 读取环境变量创建 ModelAdapter
/// 2. 通过 AgentBuilder 构建 Agent
/// 3. 运行 Agent 主循环
///
/// 多 Agent 示例：
/// ```ignore
/// let agent_a = Agent::builder().model(model_a).build()?;
/// let agent_b = Agent::builder().model(model_b).current_dir("/other").build()?;
/// let handle_a = Agent::spawn("agent-a", agent_a);
/// let handle_b = Agent::spawn("agent-b", agent_b);
/// handle_a.join().await?;
/// handle_b.join().await?;
/// ```

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let model = initial_model()?;

    let mut agent = Agent::builder()
        .model(model)
        .build()?;

    agent.run().await
}

fn initial_model() -> anyhow::Result<Box<dyn ModelAdapter>> {
    dotenvy::dotenv().ok();

    let api_key = env::var("DEEPSEEK_API_KEY")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;
    let deepseek_base_url = env::var("DEEPSEEK_BASE_URL")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_BASE_URL not set"))?;

    let openai_adapter = OpenAiCompatibleAdapter::new(
        deepseek_base_url,
        api_key,
        "deepseek-v4-flash".to_string(),
    );

    Ok(Box::new(openai_adapter))
}
