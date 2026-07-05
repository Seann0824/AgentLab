use agent_lab::{
    agent::simple_agent::SimpleAgent,
    base::{agent::Agent, config::Config, llm::AgentsLLM},
    tools::{ToolManager, memory::MemoryTool},
};

#[tokio::main]
async fn main() -> () {
    let mut agent = SimpleAgent::new(
        "情景记忆助手",
        AgentsLLM::from_env(),
        "你是一个有情景记忆的助手。当用户描述一个具体事件、经历或交互时，必须立即调用 memory 工具的 add 动作保存，memory_type 使用 episodic；当用户询问过去的事件、经历时，必须调用 memory 工具的 search 动作查找，memory_type 使用 episodic。".to_string(),
        Config::from_env(),
        ToolManager::new()
            .with_tool(Box::new(MemoryTool::new().await)),
        true,
    );

    // // 第一轮：让 agent 记录几个具体事件到情景记忆
    // println!("\n=== 第一轮：记录情景事件 ===");
    // let _ = agent.run("请记录：我上周去了杭州西湖，天气很好，拍了很多照片").await;
    // let _ = agent.run("请记录：昨天我和同事在会议室讨论了 Q4 的产品规划").await;
    // let _ = agent.run("请记录：今天早上我在地铁站帮一位老人搬了行李").await;

    // // 清空对话历史，排除模型仅靠上下文记住答案的情况
    // agent.clear_history();

    // 第二轮：测试情景记忆是否能被独立召回
    println!("\n=== 第二轮：回忆情景事件 ===");
    let _ = agent.run("我上周去了哪里？").await;

    agent.clear_history();
    let _ = agent.run("我和同事讨论了什么？").await;

    agent.clear_history();
    let _ = agent.run("今天早上我做了什么？").await;
}
