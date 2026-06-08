use tokio::sync::mpsc;

use crate::model::{ChatMessage, ModelAdapter};

use super::types::{ContextMessage, MessageImportance, SummaryResult, SummaryScope, SummaryTask};

/// ⭐ 将 ContextMessage 列表格式化为自然对话文本（用于 LLM 摘要输入）
///
/// 把 Debug 输出转为可读的对话格式，LLM 能更好地理解上下文。
fn format_messages_for_summary(messages: &[ContextMessage]) -> String {
    let mut lines = Vec::new();
    for msg in messages {
        let line = match &msg.message {
            ChatMessage::System { content } => format!("[系统]: {}", content),
            ChatMessage::User { content } => format!("[用户]: {}", content),
            ChatMessage::Assistant {
                content,
                tool_calls,
            } => {
                if tool_calls.is_empty() {
                    format!("[助手]: {}", content)
                } else {
                    let tools: Vec<String> = tool_calls
                        .iter()
                        .map(|tc| format!("  - 调用工具: {}({})", tc.name, tc.arguments))
                        .collect();
                    format!("[助手]: {}\n{}", content, tools.join("\n"))
                }
            }
            ChatMessage::Tool { content, .. } => {
                // 截断过长的工具结果
                let truncated = if content.len() > 200 {
                    format!("{}... [共 {} 字符]", &content[..200], content.len())
                } else {
                    content.clone()
                };
                format!("[工具结果]: {}", truncated)
            }
        };
        lines.push(line);
    }
    lines.join("\n")
}

/// 规则摘要生成器（非 LLM 版本，兜底方案）
///
/// 当异步摘要不可用或队列积压时使用。
/// 结构化输出，保留决策链路。
pub fn rule_based_summary(messages: &[ContextMessage]) -> String {
    let mut user_intents: Vec<String> = Vec::new();
    let mut executed_ops: Vec<String> = Vec::new();
    let mut decisions: Vec<String> = Vec::new();
    let mut key_files: Vec<String> = Vec::new();

    for ctx_msg in messages {
        match &ctx_msg.message {
            ChatMessage::User { content } => {
                let intent: String = content.chars().take(80).collect();
                if !user_intents.contains(&intent) {
                    user_intents.push(intent);
                }
            }
            ChatMessage::Tool { content, .. } => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
                    if let Some(result) = val.get("result") {
                        if let Some(stdout) = result.get("stdout").and_then(|s| s.as_str()) {
                            // 提取文件操作信息
                            if stdout.contains(".rs") || stdout.contains("Cargo.toml") {
                                for line in stdout.lines() {
                                    if line.contains(".rs") || line.contains("Cargo.toml") {
                                        let file = line.trim().to_string();
                                        if !key_files.contains(&file) {
                                            key_files.push(file);
                                        }
                                    }
                                }
                            }
                            // 记录命令执行
                            let cmd_preview: String = stdout.chars().take(60).collect();
                            if !executed_ops.contains(&cmd_preview) {
                                executed_ops.push(cmd_preview);
                            }
                        }
                    }
                }
            }
            ChatMessage::Assistant { content, .. } => {
                // 检测决策语句
                for sentence in content.split('。') {
                    let trimmed = sentence.trim();
                    if !trimmed.is_empty()
                        && (trimmed.contains("决定")
                            || trimmed.contains("选择")
                            || trimmed.contains("改为")
                            || trimmed.contains("采用"))
                    {
                        if !decisions.contains(&trimmed.to_string()) {
                            decisions.push(trimmed.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let state = if key_files.is_empty() {
        "无文件变更".to_string()
    } else {
        format!("已涉及文件: {}", key_files.join(", "))
    };

    format!(
        "【历史对话摘要】\n\
         ── 用户意图 ──\n{}\n\n\
         ── 已执行操作 ──\n{}\n\n\
         ── 关键决策 ──\n{}\n\n\
         ── 当前状态 ──\n{}",
        user_intents.join("\n"),
        executed_ops.join("\n"),
        decisions.join("\n"),
        state
    )
}

/// 异步摘要生成器
///
/// 运行在独立的 tokio task 中，通过 channel 接收任务。
/// 摘要完成后，将摘要消息通过回调注入 ContextManager。
pub struct AsyncSummarizer;

impl AsyncSummarizer {
    /// 启动后台摘要任务
    ///
    /// 返回 (task_sender, result_receiver, handle)
    /// - task_sender: 用于派发摘要任务
    /// - result_receiver: 用于接收摘要结果
    /// - handle: 后台任务的 JoinHandle
    pub fn start(
        model_adapter: Option<Box<dyn ModelAdapter>>,
    ) -> (
        mpsc::UnboundedSender<SummaryTask>,
        mpsc::UnboundedReceiver<SummaryResult>,
        tokio::task::JoinHandle<()>,
    ) {
        let (task_tx, mut task_rx) = mpsc::unbounded_channel::<SummaryTask>();
        let (result_tx, result_rx) = mpsc::unbounded_channel::<SummaryResult>();

        let handle = tokio::spawn(async move {
            while let Some(task) = task_rx.recv().await {
                // 1. 选择需要摘要的消息
                let to_summarize = match &task.scope {
                    SummaryScope::EarlyNonPreserved { keep_recent } => {
                        let mut count = 0;
                        let mut target_turns = 0;
                        // 从后往前数 keep_recent 轮（以 User 消息为标记）
                        for msg in task.messages.iter().rev() {
                            if matches!(&msg.message, ChatMessage::User { .. }) {
                                target_turns += 1;
                                if target_turns > *keep_recent {
                                    break;
                                }
                            }
                            count += 1;
                        }
                        let split_point = task.messages.len() - count;
                        task.messages[..split_point]
                            .iter()
                            .filter(|m| {
                                !m.preserved && !matches!(&m.message, ChatMessage::System { .. })
                            })
                            .cloned()
                            .collect::<Vec<_>>()
                    }
                    SummaryScope::AllNonPreserved => task
                        .messages
                        .iter()
                        .filter(|m| {
                            !m.preserved && !matches!(&m.message, ChatMessage::System { .. })
                        })
                        .cloned()
                        .collect(),
                };

                if to_summarize.is_empty() {
                    continue;
                }

                // ⭐ 记录被摘要的消息数量（用于注入后删除原始消息）
                let summarized_count = to_summarize.len();

                // 2. 生成摘要（优先 LLM，兜底规则摘要）
                let summary_text = if let Some(ref adapter) = model_adapter {
                    Self::generate_llm_summary(adapter.as_ref(), &to_summarize).await
                } else {
                    Ok(rule_based_summary(&to_summarize))
                };

                match summary_text {
                    Ok(text) => {
                        // 3. 构建摘要消息
                        let summary_message = ContextMessage {
                            message: ChatMessage::user(format!(
                                "【历史对话摘要 - 由系统自动生成】\n{}",
                                text
                            )),
                            preserved: true, // 摘要本身标记为永久保留
                            importance: MessageImportance::Important,
                        };

                        let scope_desc = match &task.scope {
                            SummaryScope::EarlyNonPreserved { keep_recent } => {
                                format!("早期对话摘要（保留最近 {} 轮）", keep_recent)
                            }
                            SummaryScope::AllNonPreserved => "全部历史摘要".to_string(),
                        };

                        // 4. 发送回主线程（携带被摘要的消息数量）
                        let _ = result_tx.send(SummaryResult {
                            summary_message,
                            scope_description: scope_desc,
                            summarized_count,
                        });
                    }
                    Err(e) => {
                        // LLM 摘要失败，用规则摘要兜底
                        let text = rule_based_summary(&to_summarize);
                        let summary_message = ContextMessage {
                            message: ChatMessage::user(format!(
                                "【历史对话摘要 - 规则生成（LLM不可用）】\n{}",
                                text
                            )),
                            preserved: true,
                            importance: MessageImportance::Important,
                        };
                        let _ = result_tx.send(SummaryResult {
                            summary_message,
                            scope_description: format!("规则摘要（LLM错误: {}）", e),
                            summarized_count,
                        });
                    }
                }
            }
        });

        (task_tx, result_rx, handle)
    }

    /// ⭐ LLM 结构化摘要生成
    ///
    /// 使用自然对话格式输入，而非 Debug 输出。
    async fn generate_llm_summary(
        model: &dyn ModelAdapter,
        messages: &[ContextMessage],
    ) -> anyhow::Result<String> {
        // ⭐ 使用自然对话格式，LLM 更容易理解
        let content = format_messages_for_summary(messages);

        let summary_prompt = ChatMessage::user(format!(
            r#"请将以下对话压缩为结构化摘要，保留关键决策和技术细节。

要求：
1. 按「目标 → 已执行操作 → 关键发现 → 当前状态」组织
2. 保留所有文件路径、命令、错误信息
3. 控制在 300 字以内
4. 如果涉及技术方案选择，标注"【决策】"

对话内容：
{}

请直接输出摘要，不要额外解释。"#,
            content
        ));

        let system_msg =
            ChatMessage::system("你是一个精准的结构化摘要助手。输出简洁，保留可操作信息。");

        let mut stream =
            model.stream_chat(&vec![system_msg, summary_prompt], serde_json::json!([]));

        use futures_util::StreamExt;
        let mut summary = String::new();
        while let Some(event) = stream.next().await {
            if let crate::model::ModelEvent::Text(text) = event {
                summary.push_str(&text);
            }
        }

        if summary.is_empty() {
            anyhow::bail!("LLM returned empty summary");
        }

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChatMessage;

    #[test]
    fn test_rule_based_summary_empty() {
        let result = rule_based_summary(&[]);
        assert!(result.contains("历史对话摘要"));
    }

    #[test]
    fn test_rule_based_summary_with_messages() {
        let messages = vec![
            ContextMessage::from(ChatMessage::user("帮我查看项目结构")),
            ContextMessage::from(ChatMessage::assistant(
                "好的，我来看看。经过分析，【决策】使用模块化架构。",
            )),
        ];

        let result = rule_based_summary(&messages);
        assert!(result.contains("帮我查看项目结构"));
        assert!(result.contains("决策"));
    }

    #[test]
    fn test_rule_based_summary_detects_files() {
        let tool_msg = ChatMessage::tool(
            "call_1",
            r#"{"ok": true, "result": {"stdout": "src/main.rs\nsrc/lib.rs\nCargo.toml\n"}}"#,
        );
        let messages = vec![ContextMessage::from(tool_msg)];

        let result = rule_based_summary(&messages);
        assert!(result.contains("main.rs"));
    }

    #[test]
    fn test_format_messages_for_summary() {
        let messages = vec![
            ContextMessage::from(ChatMessage::system("You are a helpful assistant.")),
            ContextMessage::from(ChatMessage::user("Hello!")),
            ContextMessage::from(ChatMessage::assistant("Hi there!")),
        ];

        let formatted = format_messages_for_summary(&messages);
        assert!(formatted.contains("[系统]"));
        assert!(formatted.contains("[用户]"));
        assert!(formatted.contains("[助手]"));
    }

    #[test]
    fn test_format_messages_with_tool_calls() {
        let messages = vec![ContextMessage::from(ChatMessage::assistant_tool_calls(
            "Let me check",
            vec![crate::model::ToolCall {
                id: "call_1".into(),
                name: "shell".into(),
                arguments: r#"{"command": "ls"}"#.into(),
            }],
        ))];

        let formatted = format_messages_for_summary(&messages);
        assert!(formatted.contains("调用工具"));
        assert!(formatted.contains("shell"));
    }

    #[test]
    fn test_format_messages_truncates_long_tool_output() {
        let long_content = "x".repeat(500);
        let messages = vec![ContextMessage::from(ChatMessage::tool(
            "call_1",
            &long_content,
        ))];

        let formatted = format_messages_for_summary(&messages);
        assert!(formatted.contains("[共 500 字符]"));
    }
}
