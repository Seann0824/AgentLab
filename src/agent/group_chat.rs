use crate::base::agent::Agent;

/// 当某段文本中出现指定字符串时触发终止。
pub struct TextMentionTermination(pub String);

/// 基于新 `Agent` trait 的多 Agent 轮询群聊。
///
/// 每个参与者轮流根据当前历史文本生成回复，回复会追加到共享历史中。
/// 每一轮结束后询问用户反馈，用户输入包含终止字符串时结束协作。
pub struct RoundRobinGroupChat {
    participants: Vec<Box<dyn Agent>>,
    termination: TextMentionTermination,
    max_turns: usize,
    history: String,
}

impl RoundRobinGroupChat {
    pub fn new(
        participants: Vec<Box<dyn Agent>>,
        termination: TextMentionTermination,
        max_turns: usize,
    ) -> Self {
        Self {
            participants,
            termination,
            max_turns,
            history: String::new(),
        }
    }

    /// 启动群聊循环。
    ///
    /// - `task`: 初始任务描述，会作为历史的第一条内容。
    pub async fn run(&mut self, task: &str) {
        self.history.push_str(&format!("# 任务\n{}\n", task));

        for turn in 0..self.max_turns {
            println!("\n========== 第 {} 轮 ==========", turn + 1);

            for agent in &mut self.participants {
                let name = agent.base().name.clone();
                let response = agent.run(&self.history).await;

                println!("\n## {}\n{}", name, response);

                self.history
                    .push_str(&format!("\n\n## {}\n{}", name, response));
            }

            // 每轮结束后收集用户反馈
            println!(
                "\n请输入修改意见（包含 '{}' 结束协作）：",
                self.termination.0
            );
            let mut user_input = String::new();
            let _ = std::io::stdin().read_line(&mut user_input);
            let user_input = user_input.trim();

            if user_input.contains(&self.termination.0) {
                println!("协作结束。");
                return;
            }

            self.history
                .push_str(&format!("\n\n## 用户反馈\n{}", user_input));
        }

        println!("达到最大轮数 {}，协作结束。", self.max_turns);
    }
}
