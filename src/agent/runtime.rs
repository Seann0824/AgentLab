use super::goal_loop::GoalLoopState;
use super::handle::AgentHandle;
use super::input::InputOutcome;
use super::prompt::build_system_prompt;
use super::{Agent, AgentBuilder};
use crate::context::ContextManager;

pub(super) struct RunState {
    pub(super) is_auto: bool,
    pub(super) terminal_line_dirty: bool,
    pub(super) single_task_used: bool,
    pub(super) goal_loop_state: GoalLoopState,
    pub(super) auto_extract_counter: usize,
}

impl Default for RunState {
    fn default() -> Self {
        Self {
            is_auto: false,
            terminal_line_dirty: false,
            single_task_used: false,
            goal_loop_state: GoalLoopState::default(),
            auto_extract_counter: 0,
        }
    }
}

impl Agent {
    pub fn builder() -> AgentBuilder {
        AgentBuilder::new()
    }

    pub fn spawn(name: impl Into<String>, mut agent: Agent) -> AgentHandle {
        let name = name.into();
        let task = tokio::task::spawn(async move { agent.run().await });
        AgentHandle { name, task }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let token_limit = self.config.token_limit;
        let single_task = parse_single_task();
        let goal_driven_enabled = single_task.is_none();
        let policy_summary = String::new();
        let system_prompt =
            build_system_prompt(&self.current_dir, &policy_summary, &self.tool_manager);

        self.context_manager = ContextManager::new(system_prompt, self.config.to_strategy());
        self.context_manager.setup_summary_channel(Some(
            self.model_manager
                .clone_active_adapter()
                .expect("当前模型适配器不可用"),
        ));
        self.task_manager = crate::task::TaskManager::new(&self.current_dir);

        if goal_driven_enabled && self.goal_manager.has_active_goal() {
            if let Some(goal_msg) = self.goal_manager.get_inject_message() {
                self.context_manager.add_message(goal_msg);
                eprintln!("🎯 已发现活跃目标，目标状态已注入上下文");
            }
        }
        self.task_manager.load();

        let mut state = RunState::default();
        loop {
            if !state.is_auto {
                match self.read_next_input(&single_task, &mut state).await? {
                    InputOutcome::Ready => {}
                    InputOutcome::Continue => continue,
                    InputOutcome::Exit => break Ok(()),
                }
            }

            self.prepare_context_for_turn(token_limit).await;
            let turn = self.run_model_turn(&mut state.terminal_line_dirty).await?;
            self.finish_model_turn(turn, goal_driven_enabled, &mut state)
                .await;
        }
    }
}

fn parse_single_task() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--task" {
        if args.len() > 2 {
            Some(args[2..].join(" "))
        } else {
            eprintln!("⚠️  --task 参数需要提供任务描述");
            None
        }
    } else {
        None
    }
}
