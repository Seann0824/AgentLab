use agent_lab::{
    base::llm::AgentsLLM,
    tools::{
        memory::base::get_db_client,
        rag::{RagIndex, RagTool},
    },
};
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content, MessageRole, ToolChoiceType};

#[tokio::main]
async fn main() {
    if let Err(e) = run_rag_qa_loop().await {
        eprintln!("\n❌ RAG 问答失败: {}", e);
        std::process::exit(1);
    }
}

/// 交互式 RAG 问答循环。
///
/// 启动时一次性索引 `Figma Agent：设计系统 × Agent 架构分享.md`，
/// 之后用户可反复输入问题，系统从 rag_chunks 检索相关段落并用 LLM 生成答案。
async fn run_rag_qa_loop() -> Result<(), String> {
    let db = get_db_client().await;
    let index = RagIndex::with_default_embedder(db);
    let rag_tool = RagTool::new();

    let path = "Figma Agent：设计系统 × Agent 架构分享.md";
    let namespace = "figma_agent";

    println!("=== RAG 问答系统 ===");
    println!("正在索引文档: {}", path);

    let text = rag_tool.get_markdown_content(path)?;
    if text.is_empty() {
        return Err(format!("{} 为空或读取失败", path));
    }

    let paragraphs = rag_tool.split_paragraphs_with_headings(text);
    let chunks = rag_tool.chunk_paragraphs(paragraphs, 512, 64);

    let deleted = index.clear_namespace(namespace).await?;
    if deleted > 0 {
        println!("清空旧索引: {} 条", deleted);
    }

    index
        .index_chunks(chunks.clone(), path, namespace, 8)
        .await
        .map_err(|e| format!("索引失败: {}（请确认 Ollama 已启动并加载 nomic-embed-text）", e))?;
    println!("索引完成，共 {} 个 chunk\n", chunks.len());

    let llm = AgentsLLM::from_env();

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

        let results = index
            .search(question, Some(namespace), 5)
            .await
            .map_err(|e| format!("检索失败: {}（请确认 Ollama 已启动）", e))?;

        if results.is_empty() {
            println!("未找到相关资料。\n");
            continue;
        }

        let context = results
            .iter()
            .enumerate()
            .map(|(i, (_, chunk))| {
                format!(
                    "[{}] 来源: {} | 标题路径: {}\n{}",
                    i + 1,
                    chunk.source,
                    chunk.heading_path.as_deref().unwrap_or("无"),
                    chunk.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "你是基于以下参考资料回答问题的助手。请严格根据参考资料回答，不要编造。\
             如果资料不足以回答问题，请明确说明。\n\n参考资料：\n{}\n\n用户问题：{}",
            context, question
        );

        let messages = vec![ChatCompletionMessage {
            role: MessageRole::user,
            content: Content::Text(prompt),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        match llm
            .chat_completion(messages, vec![], ToolChoiceType::None)
            .await
        {
            Ok(resp) => {
                if let Some(choice) = resp.choices.first() {
                    match &choice.message.content {
                        Some(answer) => println!("\n🤖 {}\n", answer.trim()),
                        None => println!("\n🤖 （模型未返回内容）\n"),
                    }
                } else {
                    println!("\n🤖 （模型未返回内容）\n");
                }
            }
            Err(e) => println!("\n❌ LLM 调用失败: {}\n", e),
        }
    }

    Ok(())
}
