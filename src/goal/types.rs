// src/goal/types.rs
//
// 🎯 目标驱动（Goal-Driven）能力 — 数据类型
//
// 定义 Goal 的核心数据结构，支持序列化/反序列化。
// 遵循设计文档 docs/designs/goal-driven-capability.md

use std::fmt;

/// ⭐ 目标状态枚举
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum GoalStatus {
    /// 已提议（用户刚设定，尚未激活）
    Proposed,
    /// 进行中（正在执行）
    Active,
    /// 已完成（自评估通过）
    Completed,
    /// 失败（自评估确认不可达成）
    Failed,
    /// 已取消（用户主动取消）
    Cancelled,
}

impl fmt::Display for GoalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoalStatus::Proposed => write!(f, "Proposed"),
            GoalStatus::Active => write!(f, "Active"),
            GoalStatus::Completed => write!(f, "Completed"),
            GoalStatus::Failed => write!(f, "Failed"),
            GoalStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl GoalStatus {
    /// 返回对应的 emoji 图标
    pub fn emoji(&self) -> &'static str {
        match self {
            GoalStatus::Proposed => "📋",
            GoalStatus::Active => "🚀",
            GoalStatus::Completed => "🎉",
            GoalStatus::Failed => "❌",
            GoalStatus::Cancelled => "⏹️",
        }
    }

    /// 是否为终止状态（完成后不再自动执行）
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            GoalStatus::Completed | GoalStatus::Failed | GoalStatus::Cancelled
        )
    }
}

/// ⭐ 目标定义
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Goal {
    /// 唯一标识（UUID）
    pub id: String,
    /// 目标名称（简短描述）
    pub name: String,
    /// 目标详细描述
    pub description: String,
    /// 完成标准（显式条件列表）
    pub criteria: Vec<String>,
    /// 当前状态
    pub status: GoalStatus,
    /// 执行进度 (0-100)
    pub progress: u8,
    /// 停滞检测计数（连续无进展轮次）
    pub stall_count: u32,
    /// 关联的步骤列表
    pub steps: Vec<String>,
    /// 已完成步骤
    pub completed_steps: Vec<String>,
    /// 关键决策记录
    pub decisions: Vec<String>,
    /// 创建时间
    pub created_at: String,
    /// 最后更新时间
    pub updated_at: String,
    /// 完成时间（当 status == Completed 时）
    pub completed_at: Option<String>,
}

impl Goal {
    /// 创建新 Goal
    pub fn new(name: String, description: String, criteria: Vec<String>) -> Self {
        let now = chrono_now();
        Self {
            id: generate_id(),
            name,
            description,
            criteria,
            status: GoalStatus::Proposed,
            progress: 0,
            steps: Vec::new(),
            completed_steps: Vec::new(),
            decisions: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
            stall_count: 0,
        }
    }

    /// 所有步骤是否已完成
    pub fn all_steps_done(&self) -> bool {
        if self.steps.is_empty() {
            return false;
        }
        self.completed_steps.len() >= self.steps.len()
    }

    /// 是否停滞（连续无进展轮次超过阈值）
    pub fn is_stalled(&self) -> bool {
        self.stall_count >= 5
    }

    /// 添加完成的标准（检查进度变化）
    pub fn record_step_completed(&mut self, step: String) {
        if !self.completed_steps.contains(&step) {
            self.completed_steps.push(step);
            self.stall_count = 0; // 有进展，重置停滞计数
            // 更新进度
            let total = self.steps.len();
            if total > 0 {
                self.progress = ((self.completed_steps.len() as f64 / total as f64) * 100.0) as u8;
            }
        } else {
            self.stall_count += 1; // 重复步骤，增加停滞计数
        }
        self.updated_at = chrono_now();
    }

    /// 添加决策记录
    pub fn add_decision(&mut self, decision: String) {
        self.decisions.push(decision);
        self.updated_at = chrono_now();
    }

    /// 更新状态并记录时间
    pub fn set_status(&mut self, status: GoalStatus) {
        self.status = status.clone();
        self.updated_at = chrono_now();
        if status == GoalStatus::Completed {
            self.completed_at = Some(chrono_now());
            self.progress = 100;
        }
    }

    /// 生成完成标准的文本描述
    pub fn criteria_text(&self) -> String {
        if self.criteria.is_empty() {
            return String::new();
        }
        let mut lines = Vec::new();
        for (i, c) in self.criteria.iter().enumerate() {
            lines.push(format!("  {}. {}", i + 1, c));
        }
        lines.join("\n")
    }

    /// 生成进度文本（用于注入系统提示）
    pub fn progress_text(&self) -> String {
        let pct = self.progress;
        let bar_len = 20;
        let filled = (pct as f64 / 100.0 * bar_len as f64) as usize;
        let empty = bar_len - filled;
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
        format!("[{}] {}%", bar, pct)
    }

    /// 标记 Goal 为 Active（开始执行）
    pub fn activate(&mut self) {
        self.set_status(GoalStatus::Active);
    }

    /// 标记 Goal 为 Completed
    pub fn complete(&mut self) {
        self.set_status(GoalStatus::Completed);
    }

    /// 标记 Goal 为 Failed
    pub fn fail(&mut self) {
        self.set_status(GoalStatus::Failed);
    }

    /// 标记 Goal 为 Cancelled
    pub fn cancel(&mut self) {
        self.set_status(GoalStatus::Cancelled);
    }

    /// 是否为终止状态
    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }
}

/// 生成简单的唯一 ID（基于时间戳 + 随机数）
fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let micros = dur.subsec_micros();
    // 使用一个简单的随机数
    let rand_val = (micros as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let short_id = rand_val % 100_000;
    format!("g{:x}{:05x}", secs % 0xFFFF, short_id)
}

/// 获取当前时间字符串
pub(crate) fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
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
    fn test_goal_creation() {
        let goal = Goal::new(
            "测试目标".to_string(),
            "这是一个测试目标".to_string(),
            vec!["条件1".to_string(), "条件2".to_string()],
        );
        assert_eq!(goal.status, GoalStatus::Proposed);
        assert_eq!(goal.progress, 0);
        assert_eq!(goal.criteria.len(), 2);
        assert!(!goal.id.is_empty());
    }

    #[test]
    fn test_goal_lifecycle() {
        let mut goal = Goal::new("测试".to_string(), "".to_string(), vec![]);
        assert_eq!(goal.status, GoalStatus::Proposed);

        goal.activate();
        assert_eq!(goal.status, GoalStatus::Active);
        assert!(!goal.is_terminal());

        goal.complete();
        assert_eq!(goal.status, GoalStatus::Completed);
        assert!(goal.is_terminal());
        assert!(goal.completed_at.is_some());
    }

    #[test]
    fn test_goal_progress() {
        let mut goal = Goal::new("测试".to_string(), "".to_string(), vec![]);
        goal.steps = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(goal.progress, 0);

        goal.record_step_completed("a".to_string());
        assert_eq!(goal.progress, 33);
        assert_eq!(goal.stall_count, 0);

        goal.record_step_completed("b".to_string());
        assert_eq!(goal.progress, 66);

        goal.record_step_completed("c".to_string());
        assert_eq!(goal.progress, 100);
        assert!(goal.all_steps_done());
    }

    #[test]
    fn test_stall_detection() {
        let mut goal = Goal::new("测试".to_string(), "".to_string(), vec![]);
        goal.steps = vec!["a".to_string()];
        goal.record_step_completed("a".to_string());
        assert_eq!(goal.stall_count, 0);

        // 重复记录相同步骤会增加 stall_count
        goal.record_step_completed("a".to_string());
        assert_eq!(goal.stall_count, 1);

        goal.stall_count = 5;
        assert!(goal.is_stalled());
    }
}
