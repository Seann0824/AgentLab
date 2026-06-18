struct ReflectionAgent {
    llm_client: openai::OpenaiChatCompletionClient,
    tool_manager: ToolManager,
    messages: Vec<ChatCompletionMessage>,
    max_iterations: usize,
}

impl ReflectionAgent {
    fn new(llm_client: openai::OpenaiChatCompletionClient, tool_manager: ToolManager) -> Self {
        Self {
            llm_client,
            messages: vec![],
            tool_manager,
            max_iterations: 3
        }
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
            let Some(mut code) = self.execution(&question).await else {
                break;
            };
            for i in 0..self.max_iterations {
                let Some(feedback) = self.reflection(&question, &code).await else {
                    break;
                };
                let Some(refinement_code) = self.refinement(&question, &code, &feedback).await else {
                    break;
                };
                code = refinement_code;
            }
        }
    }

    async fn execution(&mut self, task: &str) -> Option<String> {
        let prompt = format!(r#"
            你是一位资深的Python程序员。请根据以下要求，编写一个Python函数。
            你的代码必须包含完整的函数签名、文档字符串，并遵循PEP 8编码规范。

            要求: {task}

            请直接输出代码，不要包含任何额外的解释。
        "#);

        self.messages.push(
            ChatCompletionMessage {
                role: MessageRole::user,
                content: Content::Text(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None
            }
        );
        let think_stream = self.llm_client.think(self.messages.clone(), None, None).await;
        let (code, is_no_call_tool) = self.process_think_stream(Box::pin(think_stream), None).await;
        if is_no_call_tool {
            Some(code)
        } else {
            None
        }
    }

    async fn reflection(&mut self, task: &str, code: &str) -> Option<String> {
        let prompt = format!(r#"
            你是一位极其严格的代码评审专家和资深算法工程师，对代码的性能有极致的要求。
            你的任务是审查以下Python代码，并专注于找出其在<strong>算法效率</strong>上的主要瓶颈。

            # 原始任务:
            {task}

            # 待审查的代码:
            ```python
            {code}
            ```

            请分析该代码的时间复杂度，并思考是否存在一种<strong>算法上更优</strong>的解决方案来显著提升性能。
            如果存在，请清晰地指出当前算法的不足，并提出具体的、可行的改进算法建议（例如，使用筛法替代试除法）。
            如果代码在算法层面已经达到最优，才能回答“无需改进”。

            请直接输出你的反馈，不要包含任何额外的解释。
        "#);
        let messages = vec![
            ChatCompletionMessage {
                role: MessageRole::user,
                content: Content::Text(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None
            }
        ];
        let think_stream = self.llm_client.think(messages, None, None).await;
        let (feedback, is_no_call_tool) = self.process_think_stream(Box::pin(think_stream), Some(false)).await;
        if is_no_call_tool && !feedback.eq("无需改进") {
            Some(feedback)
        } else {
            None
        }
    }

    async fn refinement(&mut self, task: &str, last_code_attempt: &str, feedback: &str) -> Option<String> {
        let prompt = format!(r#"
            你是一位资深的Python程序员。你正在根据一位代码评审专家的反馈来优化你的代码。

            # 原始任务:
            {task}

            # 你上一轮尝试的代码:
            {last_code_attempt}
            评审员的反馈：
            {feedback}

            请根据评审员的反馈，生成一个优化后的新版本代码。
            你的代码必须包含完整的函数签名、文档字符串，并遵循PEP 8编码规范。
            请直接输出优化后的代码，不要包含任何额外的解释。

            请分析该代码的时间复杂度，并思考是否存在一种<strong>算法上更优</strong>的解决方案来显著提升性能。
            如果存在，请清晰地指出当前算法的不足，并提出具体的、可行的改进算法建议（例如，使用筛法替代试除法）。
            如果代码在算法层面已经达到最优，才能回答“无需改进”。

            请直接输出你的反馈，不要包含任何额外的解释。
        "#);

        self.messages.push(
            ChatCompletionMessage {
                role: MessageRole::user,
                content: Content::Text(prompt),
                name: None,
                tool_call_id: None,
                tool_calls: None
            }
        );
        let think_stream = self.llm_client.think(self.messages.clone(), None, None).await;
        let (refine, is_no_call_tool) = self.process_think_stream(Box::pin(think_stream), None).await;
        if is_no_call_tool {
            Some(refine)
        } else {
            None
        }
    }

    async fn process_think_stream(&mut self, mut think_stream: Pin<Box<impl Stream<Item = ChatCompletionStreamResponse>>>, is_remmenber: Option<bool>) -> (String, bool) {
        let is_remmenber = is_remmenber.unwrap_or(true);
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
                ChatCompletionStreamResponse::Done(_finish_reason)=> {
                    // 区分调用工具和没有调用工具的信息

                    // message 处理，工具调用处理（工具本身调用也可以作为一个流，但是本次就先做简单版本）
                    if is_remmenber {
                        self.messages.push(
                            ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(reason_delta.join("")), name: None, tool_calls: None, tool_call_id: None },
                        );
                    }
                    // tool call
                    if let Some(tools_call) = &tools_call {
                        
                        let tasks = tools_call
                            .iter()
                            .map(|tool_call| self.tool_manager.run(tool_call.clone()))
                            .collect::<Vec<_>>();
                        
                        let tools_call_result = futures_util::future::join_all(tasks).await;
                        // 工具调用
                        if is_remmenber {
                            self.messages.push(
                                ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(content_delta.join("")), tool_calls: Some(tools_call.clone()), name: None, tool_call_id: None }
                            );
                        }
                        // 工具调用结果
                        tools_call_result
                            .into_iter()
                            .for_each(|(tool_call_id, tool_call_result)| {
                                let tool_call_result = match tool_call_result {
                                    Ok(content) => content,
                                    Err(error_msg) => error_msg,
                                };
                                println!("tool_call_result: {}", tool_call_result);
                                if is_remmenber {
                                    self.messages.push(
                                        ChatCompletionMessage { role: MessageRole::tool, content: Content::Text(tool_call_result), tool_call_id: Some(tool_call_id), name: None, tool_calls: None }
                                    )
                                }
                            });     
                    } else {
                        if is_remmenber {
                            self.messages.push(
                                ChatCompletionMessage { role: MessageRole::assistant, content: Content::Text(content_delta.join("")), name: None, tool_calls: None, tool_call_id: None }
                            );
                        }
                    }
                },
            }

            std::io::Write::flush(&mut std::io::stdout());
            
        }

        (content_delta.join(""), tools_call.is_none())
    }
}
