use agent_lab::{agent::{react_agent::ReActAgent, reflection_agent::{ReflectionAgent, ReflectionPromptTemplates}, simple_agent::SimpleAgent}, base::{agent::Agent, config::Config, llm::AgentsLLM}, tools::{ToolManager, web_search::WebSearch}};

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

    // let mut react_agent = ReActAgent::new(
    //     "基础助手",
    //     AgentsLLM::from_env(),
    //     "你是一个友好的MasterGo AI助手，请用简洁明了的方式回答问题。".to_string(),
    //     Config::from_env(),
    //     ToolManager::new()
    //         .with_tool(Box::new(WebSearch::new())),
    //     5,
    // );

    let mut reflection_agent = ReflectionAgent::new(
        "基础助手",
        AgentsLLM::from_env(),
        ReflectionPromptTemplates {
            initial: "你是Python专家，请编写函数:{task}".into(),
            reflect: "请审查代码的算法效率:\n任务:{task}\n代码:{content}".into(),
            refine: "请根据反馈优化代码:\n任务:{task}\n反馈:{content}".into(),
        },
        Config::from_env(),
        ToolManager::new()
            .with_tool(Box::new(WebSearch::new())),
        5,
    );
    let _ = reflection_agent.run_reflection("帮我找到1000以内的素数").await;
}