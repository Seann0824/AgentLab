use agent_lab::{
    agent::simple_agent::SimpleAgent,
    base::{agent::Agent, config::Config, llm::AgentsLLM},
    tools::{ToolManager, memory::MemoryTool, types::Tool},
};

#[tokio::main]
async fn main() {
    if let Err(e) = run_agent_semantic_memory_test().await {
        eprintln!("\n❌ 语义记忆 Agent 测试失败: {}", e);
        std::process::exit(1);
    }
    println!("\n✅ 语义记忆 Agent 测试通过");
}

/// Agent 场景下的语义记忆端到端测试。
///
/// 通过 `SimpleAgent + MemoryTool` 完成：
/// 1. 清空历史语义记忆，避免多次运行累积重复数据；
/// 2. 记录三条用户个人事实；
/// 3. 清空对话历史，分别询问这三条事实；
/// 4. 断言回答中同时出现对应的关键信息组合。
async fn run_agent_semantic_memory_test() -> Result<(), String> {
    // 测试前置：清空当前用户的语义记忆，保证每次测试环境干净。
    let cleanup_tool = MemoryTool::new().await;
    let _ = cleanup_tool
        .execute(serde_json::json!({
            "action": "clear_all",
            "memory_type": "semantic"
        }))
        .await;

    let mut semantic_agent = SimpleAgent::new(
        "语义记忆助手",
        AgentsLLM::from_env(),
        "你是一个帮助用户记录个人事实的助手。\n\
         规则：\n\
         1. 当用户告诉你关于他自己的偏好、属性或长期事实时，必须立即调用 memory 工具的 add 动作保存，memory_type 使用 semantic。\n\
         2. 当用户询问关于他自己的事实或偏好时，必须调用 memory 工具的 search 动作查找，memory_type 使用 semantic。\n\
         3. 只基于 memory 工具返回的结果回答，不要编造。".to_string(),
        Config::from_env(),
        ToolManager::new()
            .with_tool(Box::new(MemoryTool::new().await)),
        true,
    );

    println!("\n=== 第一阶段：记录个人事实 ===");
    semantic_agent.run("请记住：我喜欢喝美式咖啡，不加糖").await;
    semantic_agent.run("请记住：我的狗叫豆豆，今年三岁了").await;
    semantic_agent.run("请记住：我每天早上 8 点起床跑步").await;

    println!("\n=== 第二阶段：通过语义记忆回答个人问题 ===");

    semantic_agent.clear_history();
    let coffee_answer = semantic_agent.run("我喜欢喝什么咖啡？").await;
    println!("\n[Q1] 我喜欢喝什么咖啡？\n{A}", A = coffee_answer);
    if !coffee_answer.contains("美式咖啡") || !coffee_answer.contains("不加糖") {
        return Err(format!("咖啡问题回答未命中关键信息: {}", coffee_answer));
    }

    semantic_agent.clear_history();
    let dog_answer = semantic_agent.run("我的狗叫什么名字？").await;
    println!("\n[Q2] 我的狗叫什么名字？\n{A}", A = dog_answer);
    if !dog_answer.contains("豆豆") || !dog_answer.contains("三岁") {
        return Err(format!("狗问题回答未命中关键信息: {}", dog_answer));
    }

    semantic_agent.clear_history();
    let morning_answer = semantic_agent.run("我早上几点起床跑步？").await;
    println!("\n[Q3] 我早上几点起床跑步？\n{A}", A = morning_answer);
    if !morning_answer.contains("8") || !morning_answer.contains("跑步") {
        return Err(format!("早晨问题回答未命中关键信息: {}", morning_answer));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_semantic_memory() {
        run_agent_semantic_memory_test()
            .await
            .expect("agent semantic memory test failed");
    }
}
