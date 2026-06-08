use super::Agent;
use crate::context::TokenEstimator;
use crate::model::ChatMessage;

impl Agent {
    pub(super) async fn prepare_context_for_turn(&mut self, token_limit: usize) {
        let injected = self.context_manager.poll_summary_results();
        let compressed = injected > 0 || self.context_manager.consume_compressed_flag();
        if injected > 0 {
            eprintln!("\r\x1b[2K📋 异步摘要已生成并注入上下文 ({} 条)", injected);
        }

        if compressed {
            self.inject_recovery_context().await;
        }

        let stats = self.context_manager.stats().clone();
        if stats.usage_ratio > 0.3 {
            eprint!(
                "\r\x1b[2K[Token: {}/{} ({:.0}%) | 保留 {} 条重要消息] ",
                TokenEstimator::format_tokens(stats.estimated_tokens),
                TokenEstimator::format_tokens(token_limit),
                stats.usage_ratio * 100.0,
                stats.preserved_count,
            );
        }

        if self.context_manager.is_blocked() {
            eprintln!(
                "\r\x1b[2K⚠️  上下文使用率 {:.0}%，触发强制压缩...",
                self.context_manager.stats().usage_ratio * 100.0,
            );
            let result = self.context_manager.force_compress();
            eprintln!(
                "\r\x1b[2K✅ 强制压缩完成: {} (tokens: {:.0}%)",
                result.description(),
                self.context_manager.stats().usage_ratio * 100.0,
            );
        } else if self.context_manager.is_critical() {
            let _ = self.context_manager.prune_tool_calls();
        }
    }

    async fn inject_recovery_context(&mut self) {
        if let Some(task_msg) = self.task_manager.get_inject_message() {
            self.context_manager.add_message(task_msg);
            eprintln!("\r\x1b[2K📋 已注入当前任务状态（帮助模型恢复上下文）");
        }
        if let Some(goal_msg) = self.goal_manager.get_inject_message() {
            self.context_manager.add_message(goal_msg);
            eprintln!("\r\x1b[2K🎯 已注入当前活跃目标状态（帮助模型持续朝着目标推进）");
        }

        let recent_messages: Vec<String> = self
            .context_manager
            .get_messages()
            .iter()
            .rev()
            .take(6)
            .filter_map(|m| match m {
                ChatMessage::User { content, .. } => Some(content.clone()),
                ChatMessage::Assistant { content, .. } if !content.is_empty() => {
                    Some(content.clone())
                }
                _ => None,
            })
            .collect();
        let query = recent_messages.join(" ");
        if query.is_empty() {
            return;
        }

        match self
            .memory_manager
            .lock()
            .await
            .search_similar(&query, 3)
            .await
        {
            Ok(results) if !results.is_empty() => {
                let mut mem_text = String::from(
                    "📌 【持久化记忆 — 检索结果】\n以下是与当前上下文相关的历史记忆：\n",
                );
                for (i, mem) in results.iter().enumerate() {
                    mem_text.push_str(&format!(
                        "{}. [相关性:{:.1}%] {}\n",
                        i + 1,
                        mem.score * 100.0,
                        mem.record.content
                    ));
                }
                self.context_manager
                    .add_message(ChatMessage::user(&mem_text));
                eprintln!(
                    "\r\x1b[2K🧠 已注入 {} 条相关持久化记忆（帮助模型恢复上下文）",
                    results.len()
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("\r\x1b[2K⚠️ 记忆检索失败: {}", e),
        }
    }
}
