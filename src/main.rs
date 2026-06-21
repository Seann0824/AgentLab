use agent_lab::{agent::simple_agent::SimpleAgent, base::{agent::Agent, config::Config, llm::AgentsLLM}, tools::{ToolManager, web_search::WebSearch}};

#[tokio::main]
async fn main() -> () {
    let mut simple_agent = SimpleAgent::new(
        "基础助手",
        AgentsLLM::from_env(),
        "你是一个友好的MasterGo AI助手，请用简洁明了的方式回答问题。".to_string(),
        Config::from_env(),
        ToolManager::new()
            .with_tool(Box::new(WebSearch::new())),
        true,
    );
    let _ = simple_agent.run("你是谁").await;
}