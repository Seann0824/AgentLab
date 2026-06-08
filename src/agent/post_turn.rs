use super::Agent;
use super::goal_loop::{GoalAutoLoopDecision, auto_detect_goal_completion, extract_goal_signal};
use super::model_turn::ModelTurn;
use super::render::{is_important_tool_result, render_tool_result_from_msg};
use super::runtime::RunState;
use crate::investigate::ErrorSnapshotManager;
use crate::memory::MemorySource;
use crate::model::ChatMessage;

const AUTO_EXTRACT_INTERVAL: usize = 3;

impl Agent {
    pub(super) async fn finish_model_turn(
        &mut self,
        turn: ModelTurn,
        goal_driven_enabled: bool,
        state: &mut RunState,
    ) {
        let ModelTurn {
            final_assistant_message,
            tool_calls,
            tool_call_ids,
            tool_results,
            has_tool_calls,
        } = turn;

        self.update_goal_status_from_response(goal_driven_enabled, &final_assistant_message);

        if has_tool_calls {
            print!("\r\x1b[K");
            for tool_result in &tool_results {
                render_tool_result_from_msg(tool_result);
            }
            self.capture_first_tool_error(&tool_calls, &tool_results);
        }

        let should_auto_extract = !has_tool_calls && !final_assistant_message.is_empty();
        let assistant_msg_clone = final_assistant_message.clone();

        if !tool_calls.is_empty() {
            self.context_manager
                .add_message(ChatMessage::assistant_tool_calls(
                    final_assistant_message,
                    tool_calls,
                ));
            state.is_auto = true;
        } else {
            self.context_manager
                .add_message(ChatMessage::assistant(final_assistant_message));
            state.is_auto = false;
        }

        self.update_goal_auto_loop(goal_driven_enabled, has_tool_calls, state);
        self.auto_extract_memory(should_auto_extract, assistant_msg_clone, state)
            .await;
        self.add_tool_results_in_order(tool_call_ids, tool_results);
    }

    fn update_goal_status_from_response(&mut self, enabled: bool, assistant_message: &str) {
        if enabled && self.goal_manager.has_active_goal() {
            if let Some(goal) = self.goal_manager.active_goal_mut() {
                let stalled = goal.is_stalled();
                let goal_id = goal.id.clone();
                let goal_clone = goal.clone();
                if stalled {
                    eprintln!(
                        "\r\x1b[2K⚠️  目标 '{}' 已停滞（连续 {} 轮无进展），自动标记为失败",
                        goal_id, goal.stall_count
                    );
                    let _ = self.goal_manager.mark_failed(&goal_id, "stalled");
                } else {
                    let _ = self.goal_manager.update(goal_clone);
                }
            }
        }

        if enabled && self.goal_manager.has_active_goal() && !assistant_message.is_empty() {
            if let Some((action, goal_id, reason)) = extract_goal_signal(assistant_message) {
                match action.as_str() {
                    "complete" => {
                        let _ = self.goal_manager.mark_complete(&goal_id);
                        println!(
                            "\n\x1b[32m━━━ ✅ LLM 自动标记目标 '{}' 为已完成 ━━━\x1b[0m 🎉",
                            goal_id
                        );
                    }
                    "fail" => {
                        let _ = self.goal_manager.mark_failed(&goal_id, &reason);
                        println!(
                            "\n\x1b[31m━━━ ❌ LLM 自动标记目标 '{}' 为失败 ━━━\x1b[0m",
                            goal_id
                        );
                    }
                    "cancel" => {
                        let _ = self.goal_manager.mark_cancelled(&goal_id);
                        println!("\n\x1b[33m━━━ 🚫 LLM 自动取消目标 '{}' ━━━\x1b[0m", goal_id);
                    }
                    _ => {}
                }
            }
        }

        if enabled && self.goal_manager.has_active_goal() && !assistant_message.is_empty() {
            let goal_id = self.goal_manager.active_goal().map(|g| g.id.clone());
            let goal_clone = self.goal_manager.active_goal().cloned();
            if let Some((ref goal_id, ref goal)) = goal_id.zip(goal_clone.as_ref()) {
                if let Some(reason) =
                    auto_detect_goal_completion(goal, assistant_message, &self.current_dir)
                {
                    let _ = self.goal_manager.mark_complete(goal_id);
                    println!(
                        "\n\x1b[32m━━━ ✅ 自动检测到目标完成信号: {} (目标 '{}' 已完成) ━━━\x1b[0m 🎉",
                        reason, goal_id
                    );
                }
            }
        }

        if enabled {
            if let Some(active_goal) = self.goal_manager.active_goal() {
                if active_goal.status.is_terminal() {
                    eprintln!(
                        "\r\x1b[2K🎯 目标 '{}' 已达终止状态 ({}), 停止自动执行",
                        active_goal.id, active_goal.status
                    );
                }
            }
        }
    }

    fn capture_first_tool_error(
        &self,
        tool_calls: &[crate::model::ToolCall],
        tool_results: &[ChatMessage],
    ) {
        let snapshot_manager = ErrorSnapshotManager::new(&self.current_dir);
        for tool_call in tool_calls {
            let Some(tool_result) = tool_results
                .iter()
                .find(|msg| msg.tool_call_id() == Some(tool_call.id.as_str()))
            else {
                continue;
            };
            let ChatMessage::Tool { content, .. } = tool_result else {
                continue;
            };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(content) else {
                continue;
            };
            if val["ok"].as_bool() != Some(false) {
                continue;
            }

            let err_msg = val["error"]["message"].as_str().unwrap_or("unknown error");
            let tool_args =
                serde_json::from_str(&tool_call.arguments).unwrap_or(serde_json::json!({}));
            let snapshot = snapshot_manager.capture(
                &self.context_manager.get_messages(),
                &tool_call.name,
                &tool_args,
                err_msg,
                None,
                0,
            );
            if let Ok(path) = snapshot_manager.save(&snapshot) {
                eprintln!(
                    "\r\x1b[2K\x1b[33m📸 错误快照已保存: {} -> {}\x1b[0m",
                    snapshot.id,
                    path.display()
                );
            }
            break;
        }
    }

    fn update_goal_auto_loop(&mut self, enabled: bool, has_tool_calls: bool, state: &mut RunState) {
        if !enabled {
            state.goal_loop_state.reset();
            return;
        }

        let goal_is_terminal = self
            .goal_manager
            .active_goal()
            .map(|goal| goal.status.is_terminal())
            .unwrap_or(false);
        match state.goal_loop_state.decide(
            self.goal_manager.has_active_goal(),
            goal_is_terminal,
            has_tool_calls,
        ) {
            GoalAutoLoopDecision::Continue => state.is_auto = true,
            GoalAutoLoopDecision::StopNoActiveGoal => {}
            GoalAutoLoopDecision::StopTerminal => state.is_auto = false,
            GoalAutoLoopDecision::StopIdle => {
                eprintln!(
                    "\r\x1b[2K🎯 目标仍为 Active，但连续 {} 轮没有工具调用；暂停自动执行，等待用户输入",
                    state.goal_loop_state.idle_iterations,
                );
                state.is_auto = false;
            }
            GoalAutoLoopDecision::StopMaxIterations => {
                eprintln!(
                    "\r\x1b[2K🎯 目标自动执行已达到 {} 轮上限；暂停自动执行，等待用户输入",
                    state.goal_loop_state.auto_iterations,
                );
                state.is_auto = false;
            }
        }
    }

    async fn auto_extract_memory(
        &mut self,
        should_auto_extract: bool,
        assistant_message: String,
        state: &mut RunState,
    ) {
        if !should_auto_extract {
            return;
        }
        state.auto_extract_counter += 1;
        if state.auto_extract_counter < AUTO_EXTRACT_INTERVAL {
            return;
        }
        state.auto_extract_counter = 0;

        let recent_msgs = self.context_manager.get_messages();
        let last_user = recent_msgs.iter().rev().find_map(|m| match m {
            ChatMessage::User { content, .. } => Some(content.clone()),
            _ => None,
        });
        if let Some(user_input) = last_user {
            let memory_content = format!(
                "[对话] 用户: {} | 助手: {}",
                truncate_chars(&user_input, 200),
                truncate_chars(&assistant_message, 500),
            );
            let tags = vec!["auto-extracted".to_string(), "conversation".to_string()];
            match self
                .memory_manager
                .lock()
                .await
                .save(&memory_content, &tags, MemorySource::Conversation, 0.3)
                .await
            {
                Ok(id) => eprintln!("\r\x1b[2K🧠 自动提取并保存了一条对话记忆 (id: {})", id),
                Err(e) => eprintln!("\r\x1b[2K⚠️ 自动提取记忆失败: {}", e),
            }
        }
    }

    fn add_tool_results_in_order(
        &mut self,
        tool_call_ids: Vec<String>,
        mut tool_results: Vec<ChatMessage>,
    ) {
        for tool_call_id in tool_call_ids {
            if let Some(index) = tool_results
                .iter()
                .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
            {
                let tool_result = tool_results.remove(index);
                let is_important = is_important_tool_result(&tool_result);
                self.context_manager.add_message(tool_result);
                if is_important {
                    self.context_manager.preserve_last_message();
                }
            }
        }
        for tool_result in tool_results {
            self.context_manager.add_message(tool_result);
        }
    }
}

fn truncate_chars(value: &str, max: usize) -> &str {
    if value.len() > max {
        &value[..max]
    } else {
        value
    }
}
