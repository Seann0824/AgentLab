const MAX_GOAL_AUTO_ITERATIONS: usize = 30;
const MAX_GOAL_IDLE_ITERATIONS: usize = 3;

#[derive(Debug, Default)]
pub(super) struct GoalLoopState {
    pub(super) auto_iterations: usize,
    pub(super) idle_iterations: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum GoalAutoLoopDecision {
    Continue,
    StopNoActiveGoal,
    StopTerminal,
    StopIdle,
    StopMaxIterations,
}

impl GoalLoopState {
    pub(super) fn reset(&mut self) {
        self.auto_iterations = 0;
        self.idle_iterations = 0;
    }

    pub(super) fn decide(
        &mut self,
        has_active_goal: bool,
        goal_is_terminal: bool,
        has_tool_calls: bool,
    ) -> GoalAutoLoopDecision {
        if !has_active_goal {
            self.reset();
            return GoalAutoLoopDecision::StopNoActiveGoal;
        }

        if goal_is_terminal {
            self.reset();
            return GoalAutoLoopDecision::StopTerminal;
        }

        self.auto_iterations += 1;
        if has_tool_calls {
            self.idle_iterations = 0;
        } else {
            self.idle_iterations += 1;
        }

        if self.auto_iterations > MAX_GOAL_AUTO_ITERATIONS {
            return GoalAutoLoopDecision::StopMaxIterations;
        }

        if self.idle_iterations > MAX_GOAL_IDLE_ITERATIONS {
            return GoalAutoLoopDecision::StopIdle;
        }

        GoalAutoLoopDecision::Continue
    }
}

/// ⭐ 从 LLM 输出文本中提取 Goal 完成信号
///
/// 解析 `/goal complete <id>`、`/goal fail <id> [reason]`、`/goal cancel <id>` 模式。
/// 返回 (action, id, reason) 三元组。
pub(super) fn extract_goal_signal(text: &str) -> Option<(String, String, String)> {
    // 匹配模式：/goal complete <id>
    //            /goal fail <id> [reason]
    //            /goal cancel <id>
    let re = regex::Regex::new(r"/goal\s+(complete|fail|cancel)\s+(\S+)(?:\s+(.*))?").ok()?;
    if let Some(caps) = re.captures(text) {
        let action = caps.get(1)?.as_str().to_string();
        let goal_id = caps.get(2)?.as_str().to_string();
        let reason = caps
            .get(3)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        Some((action, goal_id, reason))
    } else {
        None
    }
}

/// ⭐ 自动检测 Goal 完成信号（当 LLM 忘记输出 /goal complete 时的 fallback）
///
/// 检测策略：
/// 1. PLAN.md 中所有 `- [ ]` 步骤是否都已变为 `- [x]`
/// 2. AI 回复中是否包含强烈的完成信号关键词（中英文）
/// 3. Goal 本身的 all_steps_done 属性
/// 4. Goal 进度是否已达 100%
///
/// 返回 Some(reason) 如果检测到完成信号，否则返回 None。
pub(super) fn auto_detect_goal_completion(
    goal: &crate::goal::Goal,
    assistant_message: &str,
    current_dir: &str,
) -> Option<String> {
    let mut reasons = Vec::new();

    // ── 策略1: 检查 PLAN.md 是否所有步骤已完成 ──
    let plan_path = std::path::Path::new(current_dir).join("PLAN.md");
    if plan_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&plan_path) {
            let has_unchecked = content
                .lines()
                .any(|line| line.contains("- [ ]") || line.contains("- [ ]"));
            let has_any_step = content.lines().any(|line| line.contains("- ["));
            if has_any_step && !has_unchecked {
                reasons.push("PLAN.md 所有步骤已完成".to_string());
            }
        }
    }

    // ── 策略2: 检测 AI 回复中的完成关键词 ──
    if !assistant_message.is_empty() {
        let lower_msg = assistant_message.to_lowercase();
        let completion_keywords = [
            // 中文完成信号
            "已完成",
            "全部完成",
            "任务完成",
            "所有步骤已完成",
            "已完成所有",
            "全部已完成",
            "已完成全部",
            "所有步骤均已",
            "已全部完成",
            // 英文完成信号
            "all steps completed",
            "task completed",
            "all done",
            "all tasks completed",
            "completed successfully",
            "finished all",
            "all steps are done",
            // 混合模式
            "✅ 已完成",
            "✅ 全部完成",
        ];
        for kw in &completion_keywords {
            if lower_msg.contains(&kw.to_lowercase()) {
                reasons.push(format!("AI 回复包含完成关键词: {}", kw));
                break;
            }
        }
    }

    // ── 策略3: 检查 Goal 本身的 all_steps_done ──
    if goal.all_steps_done() {
        reasons.push("Goal 步骤全部标记为已完成".to_string());
    }

    // ── 策略4: 检查 progress 是否已达 100% ──
    if goal.progress >= 100 {
        reasons.push(format!("Goal 进度已达 100%"));
    }

    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("; "))
    }
}
