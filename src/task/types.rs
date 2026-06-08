// src/task/types.rs
//
// 结构化任务执行框架 — 数据类型
//
// 定义任务状态的结构化表示，支持序列化/反序列化。
// 这些类型与 PLAN.md / AGENDA.md / MEMORY.md 的内容对应。

/// ⭐ 任务状态（完整快照）
///
/// 每次 save() 时刷新到文件，load() 时从文件恢复。
/// 字段说明：
/// - `current_task`: 当前任务名称（如"实现用户登录功能"）
/// - `current_step`: 当前正在执行的步骤描述
/// - `completed_steps`: 已完成步骤列表
/// - `pending_steps`: 待完成步骤列表
/// - `important_facts`: 执行过程中发现的重要信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskState {
    pub current_task: Option<String>,
    pub current_step: Option<String>,
    pub completed_steps: Vec<String>,
    pub pending_steps: Vec<String>,
    pub important_facts: Vec<String>,
    pub last_updated: String,
}

impl Default for TaskState {
    fn default() -> Self {
        Self {
            current_task: None,
            current_step: None,
            completed_steps: Vec::new(),
            pending_steps: Vec::new(),
            important_facts: Vec::new(),
            last_updated: chrono_now(),
        }
    }
}

impl TaskState {
    /// 是否处于空闲状态（没有活跃任务）
    pub fn is_idle(&self) -> bool {
        self.current_task.is_none()
    }

    /// 已完成百分比（0~100）
    pub fn progress_pct(&self) -> u8 {
        let total = self.completed_steps.len() + self.pending_steps.len();
        if total == 0 {
            return 0;
        }
        ((self.completed_steps.len() as f64 / total as f64) * 100.0) as u8
    }

    /// 生成当前状态的纯文本摘要（注入到上下文用）
    pub fn to_context_prompt(&self) -> String {
        let mut lines = Vec::new();

        lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
        lines.push("📋 【当前任务状态】".to_string());
        lines.push(String::new());

        if let Some(task) = &self.current_task {
            lines.push(format!("  任务: {}", task));
            lines.push(format!(
                "  进度: {}% ({}/{})",
                self.progress_pct(),
                self.completed_steps.len(),
                self.completed_steps.len() + self.pending_steps.len(),
            ));
            lines.push(String::new());
        }

        if !self.completed_steps.is_empty() {
            lines.push("  ✅ 已完成:".to_string());
            for step in &self.completed_steps {
                lines.push(format!("    - [x] {}", step));
            }
            lines.push(String::new());
        }

        if !self.pending_steps.is_empty() {
            lines.push("  ⏳ 待完成:".to_string());
            for step in &self.pending_steps {
                lines.push(format!("    - [ ] {}", step));
            }
            if let Some(current) = &self.current_step {
                lines.push(format!("  ▶️  当前步骤: {}", current));
            }
            lines.push(String::new());
        }

        if !self.important_facts.is_empty() {
            lines.push("  🧠 重要发现:".to_string());
            for fact in &self.important_facts {
                lines.push(format!("    - {}", fact));
            }
            lines.push(String::new());
        }

        lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());

        lines.join("\n")
    }
}

/// 获取当前时间字符串（用于 last_updated）
pub(crate) fn chrono_now() -> String {
    // 使用 chrono 不是必须的，这里用简单方式
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // 简单格式化：YYYY-MM-DD HH:MM
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;

    // 计算大致年份（从 1970 开始）
    let year = 1970 + (days as f64 / 365.25) as u64;
    let month = 1 + ((days as f64 % 365.25) / 30.44) as u64;
    let day = 1 + (days as f64 % 30.44) as u64;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        year,
        month.min(12),
        day.min(31),
        hours,
        minutes
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_state_default() {
        let state = TaskState::default();
        assert!(state.is_idle());
        assert_eq!(state.progress_pct(), 0);
    }

    #[test]
    fn test_progress_pct() {
        let mut state = TaskState::default();
        state.current_task = Some("test".to_string());
        state.completed_steps.push("step1".to_string());
        state.pending_steps.push("step2".to_string());
        assert_eq!(state.progress_pct(), 50);
    }

    #[test]
    fn test_context_prompt_non_empty() {
        let mut state = TaskState::default();
        state.current_task = Some("实现功能X".to_string());
        state.completed_steps.push("分析需求".to_string());
        state.completed_steps.push("设计接口".to_string());
        state.pending_steps.push("编写代码".to_string());
        state.pending_steps.push("测试验证".to_string());
        state.important_facts.push("使用 Rust 实现".to_string());

        let prompt = state.to_context_prompt();
        assert!(prompt.contains("实现功能X"));
        assert!(prompt.contains("50%"));
        assert!(prompt.contains("分析需求"));
        assert!(prompt.contains("编写代码"));
        assert!(prompt.contains("使用 Rust 实现"));
    }

    #[test]
    fn test_context_prompt_idle() {
        let state = TaskState::default();
        let prompt = state.to_context_prompt();
        // 只有框架标记，没有任务信息
        assert!(prompt.contains("当前任务状态"));
    }
}
