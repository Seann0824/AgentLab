use agent_lab::{agent::{react_agent::ReActAgent, simple_agent::SimpleAgent}, base::{agent::Agent, config::Config, llm::AgentsLLM}, tools::{ToolManager, web_search::WebSearch}};

#[tokio::main]
async fn main() -> () {
    // let mut simple_agent = SimpleAgent::new(
    //     "基础助手",
    //     AgentsLLM::from_env(),
    //     "你是一个友好的MasterGo AI助手，请用简洁明了的方式回答问题。".to_string(),
    //     Config::from_env(),
    //     ToolManager::new()
    //         .with_tool(Box::new(WebSearch::new())),
    //     true,
    // );

    let mut react_agent = ReActAgent::new(
        "基础助手",
        AgentsLLM::from_env(),
        "你是一个友好的MasterGo AI助手，请用简洁明了的方式回答问题。".to_string(),
        Config::from_env(),
        ToolManager::new()
            .with_tool(Box::new(WebSearch::new())),
        5,
    );
    let _ = react_agent.run("帮我找找肖元彪都有哪些人").await;
}