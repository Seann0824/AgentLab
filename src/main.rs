use std::env;
use std::fmt::format;
use std::pin::Pin;
use dotenvy;
use futures_util::Stream;
use openai_api_rs::v1::chat_completion::chat_completion_stream::ChatCompletionStreamResponse;
use openai_api_rs::v1::chat_completion::{ChatCompletionMessage, Content};

use openai_api_rs::v1::chat_completion::MessageRole;

use crate::tools::web_search::WebSearch;
use crate::tools::ToolManager;
mod model;
mod tools;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> () {
    dotenvy::dotenv().ok();
    let api_key = env::var("DEEPSEEK_API_KEY").unwrap();
    let base_url = env::var("DEEPSEEK_BASE_URL").unwrap();
    let tool_manager = ToolManager::new()
        .register_tool(Box::new(WebSearch::new()));
    let llm_client = model::AgentLLM::new("deepseek-v4-pro", &api_key, &base_url, None);

    let mut agent = PlanAdnSolveAgent::new(llm_client, tool_manager);
    
    agent.run().await;
}

struct PlanAdnSolveAgent {
    llm_client: model::AgentLLM,
    tool_manager: ToolManager,
    messages: Vec<ChatCompletionMessage>,
    plans: Option<Vec<String>>,
}

impl PlanAdnSolveAgent {
    fn new(llm_client: model::AgentLLM, tool_manager: ToolManager) -> Self {
        Self {
            llm_client,
            messages: vec![],
            tool_manager,
            plans: None
        }
    }

    async fn plan(&mut self, question: &str) {
        let prompt = format!(r#"
            你是一个顶级的AI规划专家。你的任务是将用户提出的复杂问题分解成一个由多个简单步骤组成的行动计划。
            请确保计划中的每个步骤都是一个独立的、可执行的子任务，并且严格按照逻辑顺序排列。
            你的输出必须是一个Python列表，其中每个元素都是一个描述子任务的字符串。

            问题: {question}

            请严格按照以下格式输出你的计划,```python与```作为前后缀是必要的:
            ```rust
            ["步骤1", "步骤2", "步骤3", ...]
            ```
        "#);
        
        // build messages
        self.messages.push(
            ChatCompletionMessage {
                role: MessageRole::user,
                content: Content::Text(prompt),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }
        );

        let think_stream = self.llm_client.think(self.messages.clone(), Some(self.tool_manager.get_tools_scehma()), None).await;
        let (plan_msg, is_plan) = self.process_think_stream(Box::pin(think_stream)).await;
        if is_plan {
            self.plans = self.parser_plan(&plan_msg);
        }
    }

    async fn process_think_stream(&mut self, mut think_stream: Pin<Box<impl Stream<Item = ChatCompletionStreamResponse>>>) -> (String, bool) {
        let (mut is_first_print_content, mut is_first_print_reason) = (true, true);

        let (mut reason_delta, mut content_delta) = (vec![], vec![]);
        let mut tools_call = None; 

        while let Some(chunck) = think_stream.next().await {
            match chunck {
                ChatCompletionStreamResponse::Content(delta) => {
                    if is_first_print_content {
                        println!("\n\nAI: ");
                        is_first_print_content = false;
                    }
                    print!("{}", delta);
                    content_delta.push(delta);

                },
                ChatCompletionStreamResponse::Reasoning(delta) => {
                    if is_first_print_reason {
                        // println!("\n\nTHINK: ");
                        is_first_print_reason = false;
                    }
                    // print!("{}", delta);
                    reason_delta.push(delta);
                },
                ChatCompletionStreamResponse::ToolCall(tc) => {
                    tools_call = Some(tc);
                },
                ChatCompletionStreamResponse::Done=> {
                    // 区分调用工具和没有调用工具的信息

                    // message 处理，工具调用处理（工具本身调用也可以作为一个流，但是本次就先做简单版本）
                    self.messages.push(
                        ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(reason_delta.join("")), name: None, tool_calls: None, tool_call_id: None },
                    );
                    // tool call
                    if let Some(tools_call) = &tools_call {
                        
                        let tasks = tools_call
                            .iter()
                            .map(|tool_call| self.tool_manager.run(tool_call.clone()))
                            .collect::<Vec<_>>();
                        
                        let tools_call_result = futures_util::future::join_all(tasks).await;
                        // 工具调用
                        self.messages.push(
                            ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(content_delta.join("")), tool_calls: Some(tools_call.clone()), name: None, tool_call_id: None }
                        );
                        // 工具调用结果
                        tools_call_result
                            .into_iter()
                            .for_each(|(tool_call_id, tool_call_result)| {
                                let tool_call_result = match tool_call_result {
                                    Ok(content) => content,
                                    Err(error_msg) => error_msg,
                                };
                                println!("tool_call_result: {}", tool_call_result);
                                self.messages.push(
                                    ChatCompletionMessage { role: MessageRole::tool, content: Content::Text(tool_call_result), tool_call_id: Some(tool_call_id), name: None, tool_calls: None }
                                )
                            });     
                    } else {
                        self.messages.push(
                            ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(content_delta.join("")), name: None, tool_calls: None, tool_call_id: None }
                        );
                    }
                },
            }

            std::io::Write::flush(&mut std::io::stdout());
            
        }

        (content_delta.join(""), tools_call.is_none())
    }

    fn parser_plan(&self, plan_msg: &str) -> Option<Vec<String>> {
        let plan_reg = regex::Regex::new(r#"(?s)\[[^\]]*\]"#).unwrap();
        if let Some(plan) = plan_reg.find(plan_msg) && let Ok(plans) = serde_json::from_str::<Vec<String>>(plan.as_str()) {
            Some(plans)
        } else {
            None
        }
    }

    async fn execute(&mut self, question: &str) {
        let Some(plans) = self.plans.clone() else {
            return;
        };

        let mut history = "无".to_string();
        let whole_plan = serde_json::json!(plans);
        for (i, step) in plans.iter().enumerate() {
            let prompt = format!(r#"
                你是一位顶级的AI执行专家。你的任务是严格按照给定的计划，一步步地解决问题。
                你将收到原始问题、完整的计划、以及到目前为止已经完成的步骤和结果。
                请你专注于解决“当前步骤”，并仅输出该步骤的最终答案，不要输出任何额外的解释或对话。

                # 原始问题:
                {question}

                # 完整计划:
                {whole_plan}

                # 历史步骤与结果:
                {history}

                # 当前步骤:
                {step}

                请仅输出针对“当前步骤”的回答:
            "#);

            self.messages.push(ChatCompletionMessage {
                role: MessageRole::user,
                content: Content::Text(prompt),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });


            let think_stream = self.llm_client.think(self.messages.clone(), Some(self.tool_manager.get_tools_scehma()), None).await;
            let (step_result, _) = self.process_think_stream(Box::pin(think_stream)).await;
            
            history.push_str(&format!(
                "\n\n步骤{}: {}\n结果: {}",
                i + 1,
                step,
                step_result
            ));
        }
        self.plans = None;    
    }

    async fn run(&mut self) {
        loop {
            let mut question = String::new();
            if self.messages.is_empty() || self.messages.last().is_some_and(|last_message| last_message.role != MessageRole::tool) {
                println!("\nUser: ");
                std::io::stdin()
                    .read_line(&mut question)
                    .unwrap();
            }

            self.plan(&question).await;
            self.execute(&question).await;
        }
    }
}