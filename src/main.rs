use agent_lab::{agent::simple_agent::SimpleAgent, base::{agent::Agent, config::Config, llm::AgentsLLM}, tools::{ToolManager, memory::MemoryTool}};

#[tokio::main]
async fn main() -> () {
    let mut agent = SimpleAgent::new(
        "记忆助手",
        AgentsLLM::from_env(),
        "你是一个有长期记忆的助手。当用户提到自己的关键信息（如姓名、偏好、重要事实）时，必须立即调用 memory 工具的 add 动作保存(目前只支持working_memory, 其他的不要调用)；当用户询问之前提到过的信息时，必须调用 memory 工具的 search 动作查找，并根据搜索结果回答。".to_string(),
        Config::from_env(),
        ToolManager::new()
            .with_tool(Box::new(MemoryTool::new().await)),
        true,
    );

    // 第一轮：让 agent 记住一个事实
    println!("\n=== 第一轮：记住事实 ===");
    let _ = agent.run("请记住我最喜欢的颜色是蓝色").await;

    let _ = agent.run("我今年18岁哦").await;

    let _ = agent.run("我叫 sean，我的职业是一个前端工程师").await;

    // 清空对话历史，排除模型仅靠上下文记住答案的情况
    agent.clear_history();

    // 第二轮：测试记忆是否能被独立召回
    println!("\n=== 第二轮：回忆事实 ===");
    let _ = agent.run("sean 的信息是啥").await;
    agent.run("我最喜欢的食物是螺蛳粉").await;

    agent.clear_history();

    agent.run("我最喜欢的食物是什么？").await;
}
