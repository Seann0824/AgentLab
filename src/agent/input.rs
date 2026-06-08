use std::io::Write;

use super::Agent;
use super::goal_command::handle_goal_command;
use super::input::InputOutcome::{Continue, Exit, Ready};
use super::model_command::handle_model_command;
use super::render::finish_terminal_line;
use super::runtime::RunState;
use super::session_command::handle_session_command;
use super::swarm_command::handle_swarm_command;
use crate::investigate::ErrorSnapshotManager;
use crate::model::ChatMessage;

pub(super) enum InputOutcome {
    Ready,
    Continue,
    Exit,
}

impl Agent {
    pub(super) async fn read_next_input(
        &mut self,
        single_task: &Option<String>,
        state: &mut RunState,
    ) -> anyhow::Result<InputOutcome> {
        if let Some(task) = single_task {
            return Ok(self.read_single_task_input(task, state));
        }

        let Some(input_str) = read_interactive_input(&mut state.terminal_line_dirty)? else {
            return Ok(Continue);
        };

        if input_str.starts_with('/') {
            self.handle_slash_command(&input_str).await?;
            return Ok(Continue);
        }

        self.context_manager
            .add_message(ChatMessage::user(&input_str));
        self.task_manager.on_user_input(&input_str);
        state.goal_loop_state.reset();
        Ok(Ready)
    }

    fn read_single_task_input(&mut self, task: &str, state: &mut RunState) -> InputOutcome {
        if state.single_task_used {
            let snapshot_manager = ErrorSnapshotManager::new(&self.current_dir);
            if let Ok(snapshots) = snapshot_manager.list() {
                if let Some(last) = snapshots.last() {
                    eprintln!("[SNAPSHOT] {}", last.id);
                }
            }
            return Exit;
        }

        state.single_task_used = true;
        eprintln!("[子 agent] 执行任务: {}", task);
        self.context_manager.add_message(ChatMessage::user(task));
        self.task_manager.on_user_input(task);
        state.goal_loop_state.reset();
        Ready
    }

    async fn handle_slash_command(&mut self, input: &str) -> anyhow::Result<()> {
        let trimmed = input.trim();
        let cmd_name = trimmed
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");

        match cmd_name {
            "" => self.command_registry.print_help_short(),
            "help" | "h" | "?" => self.command_registry.print_help_full(),
            "clear" => {
                self.context_manager.clear();
                println!("\x1b[32m━━━ 🧹 历史消息已清空 ━━━\x1b[0m");
            }
            "session" | "sessions" => handle_session_command(
                trimmed,
                &self.session_manager,
                &mut self.context_manager,
                &mut self.task_manager,
            ),
            "tools" => self.print_tools(),
            "debug" => handle_debug_command(trimmed),
            "model" => handle_model_command(trimmed, &mut self.model_manager),
            "goal" => {
                handle_goal_command(trimmed, &mut self.goal_manager);
                if self.goal_manager.has_active_goal() {
                    if let Some(goal_msg) = self.goal_manager.get_inject_message() {
                        self.context_manager.add_message(goal_msg);
                        eprintln!("\r\x1b[2K🎯 目标已注入上下文，AI 将感知到当前目标");
                    }
                }
            }
            "swarm" => handle_swarm_command(trimmed, self.swarm_registry.clone()).await,
            _ if self.command_registry.is_known(cmd_name) => {
                if let Some(cmd) = self.command_registry.get(cmd_name) {
                    self.command_registry.print_command_help(cmd);
                }
            }
            _ => self.command_registry.print_unknown_command(trimmed),
        }

        Ok(())
    }

    fn print_tools(&self) {
        let tools = self.tool_manager.list_tools();
        println!("\x1b[36m━━━ 🔧 可用工具 (共 {}) ━━━\x1b[0m", tools.len());
        for t in &tools {
            println!("  \x1b[33m{:<15}\x1b[0m {}", t.name, t.description);
        }
        println!("\x1b[90m  💡 工具详情由 LLM function calling schema 自动提供\x1b[0m");
    }
}

fn read_interactive_input(terminal_line_dirty: &mut bool) -> anyhow::Result<Option<String>> {
    let mut user_input = String::new();
    finish_terminal_line(terminal_line_dirty);
    print!(">");
    std::io::stdout().flush()?;
    if std::io::stdin().read_line(&mut user_input).is_err() {
        return Ok(None);
    }
    let input = user_input.trim().to_string();
    if input.is_empty() {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

fn handle_debug_command(input: &str) {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let sub = parts.get(1).copied().unwrap_or("status");
    match sub {
        "on" | "enable" | "1" | "true" => {
            crate::debug::enable();
            println!("\x1b[32m━━━ 🐛 debug 模式已开启 ━━━\x1b[0m");
        }
        "off" | "disable" | "0" | "false" => {
            crate::debug::disable();
            println!("\x1b[33m━━━ 🐛 debug 模式已关闭 ━━━\x1b[0m");
        }
        "toggle" | "t" => {
            let new_state = crate::debug::toggle();
            if new_state {
                println!("\x1b[32m━━━ 🐛 debug 模式已切换为开启 ━━━\x1b[0m");
            } else {
                println!("\x1b[33m━━━ 🐛 debug 模式已切换为关闭 ━━━\x1b[0m");
            }
        }
        _ => {
            println!("\x1b[36m━━━ 🐛 Debug 状态 ━━━\x1b[0m");
            println!("  {}", crate::debug::status_text());
            println!("\x1b[90m  用法: /debug on|off|toggle|status\x1b[0m");
        }
    }
}
