use agent_lab_core::{
    agent::Agent,
    base::{agent::Agent as _, config::Config, llm::AgentsLLM},
    db::get_db_client,
    tools::{rag::RagTool},
};

#[tokio::main]
async fn main() {
    if let Err(e) = run_rag_agent_loop().await {
        eprintln!("\n❌ RAG Agent 失败: {}", e);
        std::process::exit(1);
    }
}

/// 交互式 RAG Agent 问答循环。
///
/// Agent 持有 `rag` 工具，可自行决定调用 search 检索资料库并基于结果回答。
/// 启动时先索引一次文档，之后用户反复输入问题即可。
async fn run_rag_agent_loop() -> Result<(), String> {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL not set".to_string())?;
    let db = get_db_client(&database_url).await;
    let llm = AgentsLLM::builder()
        .api_key(std::env::var("API_KEY").map_err(|_| "API_KEY not set".to_string())?)
        .base_url(std::env::var("BASE_URL").map_err(|_| "BASE_URL not set".to_string())?)
        .model(std::env::var("MODEL").map_err(|_| "MODEL not set".to_string())?)
        .provider(std::env::var("PROVIDER").unwrap_or_else(|_| "Custom".into()))
        .build();

    let rag_tool = RagTool::with_default_embedder(db, llm.clone());

    let namespace = "figma_agent";

    println!("=== RAG Agent 问答系统 ===");
    println!("资料库 namespace: {}\n", namespace);

    let config = Config::builder()
        .debug(std::env::var("DEBUG").map(|v| v == "true").unwrap_or(false))
        .build();

    let system_prompt = "你是 FigmaAgent 助手，专门回答关于 Figma Agent 设计系统与 Agent 架构的问题。\
        当用户询问文档相关内容时，你必须调用 `rag` 工具的 `search` action，\
        传入用户问题获取参考资料，然后基于资料回答。\
        不要编造资料中没有的内容。"
        .to_string();

    let mut agent = Agent::builder()
        .name("RAGAgent")
        .llm(llm)
        .system_prompt(system_prompt)
        .config(config)
        .tool(Box::new(rag_tool))
        .enable_tool_calling(true)
        .build();

    println!("请输入问题（空行 / quit / exit 退出）：\n");

    loop {
        print!("> ");
        if let Err(e) = std::io::Write::flush(&mut std::io::stdout()) {
            return Err(format!("flush stdout failed: {}", e));
        }

        let mut question = String::new();
        if let Err(e) = std::io::stdin().read_line(&mut question) {
            return Err(format!("read stdin failed: {}", e));
        }
        let question = question.trim();

        if question.is_empty() || question == "quit" || question == "exit" {
            println!("再见！");
            break;
        }

        let _answer = agent.run(question).await;
        println!();
    }

    Ok(())
}
