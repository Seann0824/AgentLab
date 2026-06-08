// src/goal/registry.rs
//
// 🎯 GoalRegistry — 目标持久化存储
//
// 负责：
// 1. 将 Goal 数据持久化到 docs/goals/ 目录
// 2. 提供创建、读取、更新、列出、删除 Goal 的接口
// 3. 维护 index.json 索引文件

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::types::{Goal, GoalStatus, chrono_now};

/// Goal 索引条目（轻量摘要）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalIndexEntry {
    pub id: String,
    pub name: String,
    pub status: String,
    pub progress: u8,
    pub updated_at: String,
}

/// Goal 索引文件结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct GoalIndex {
    goals: Vec<GoalIndexEntry>,
    last_updated: String,
}

/// ⭐ Goal 注册表 — 持久化存储
pub struct GoalRegistry {
    /// 存储目录
    goals_dir: PathBuf,
    /// 内存中的 Goal 缓存（避免频繁磁盘 IO）
    goals: HashMap<String, Goal>,
}

impl GoalRegistry {
    /// 创建新的 GoalRegistry，指定存储根目录
    pub fn new(root_dir: impl Into<String>) -> Self {
        let root = PathBuf::from(root_dir.into());
        let goals_dir = root.join("docs").join("goals");
        Self {
            goals_dir,
            goals: HashMap::new(),
        }
    }

    /// 确保存储目录存在
    fn ensure_dir(&self) -> anyhow::Result<()> {
        if !self.goals_dir.exists() {
            std::fs::create_dir_all(&self.goals_dir)?;
        }
        Ok(())
    }

    /// 获取 Goal 文件的路径
    fn goal_path(&self, id: &str) -> PathBuf {
        self.goals_dir.join(format!("goal_{}.json", id))
    }

    /// 获取索引文件路径
    fn index_path(&self) -> PathBuf {
        self.goals_dir.join("index.json")
    }

    /// ⭐ 从磁盘加载所有 Goal（启动时调用）
    pub fn load_all(&mut self) -> anyhow::Result<()> {
        if !self.goals_dir.exists() {
            return Ok(());
        }

        // 从 index.json 读取列表
        let index_path = self.index_path();
        let goal_ids: Vec<String> = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path)?;
            let index: GoalIndex = serde_json::from_str(&content)?;
            index.goals.iter().map(|e| e.id.clone()).collect()
        } else {
            // fallback: 从目录中扫描 goal_*.json 文件
            let mut ids = Vec::new();
            for entry in std::fs::read_dir(&self.goals_dir)? {
                let entry = entry?;
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("goal_") && name_str.ends_with(".json") {
                    let id = name_str
                        .trim_start_matches("goal_")
                        .trim_end_matches(".json")
                        .to_string();
                    ids.push(id);
                }
            }
            ids
        };

        // 加载每个 Goal
        for id in goal_ids {
            let path = self.goal_path(&id);
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        if let Ok(goal) = serde_json::from_str::<Goal>(&content) {
                            self.goals.insert(id, goal);
                        }
                    }
                    Err(e) => {
                        eprintln!("⚠️  无法读取 Goal {}: {}", id, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// ⭐ 保存索引文件
    fn save_index(&self) -> anyhow::Result<()> {
        let entries: Vec<GoalIndexEntry> = self.goals.values().map(|g| {
            GoalIndexEntry {
                id: g.id.clone(),
                name: g.name.clone(),
                status: g.status.to_string(),
                progress: g.progress,
                updated_at: g.updated_at.clone(),
            }
        }).collect();

        let index = GoalIndex {
            goals: entries,
            last_updated: chrono_now(),
        };

        let content = serde_json::to_string_pretty(&index)?;
        std::fs::write(self.index_path(), content)?;
        Ok(())
    }

    /// ⭐ 创建新 Goal（持久化到文件）
    pub fn create(&mut self, goal: Goal) -> anyhow::Result<()> {
        self.ensure_dir()?;

        let id = goal.id.clone();
        let path = self.goal_path(&id);

        // 写入 Goal 文件
        let content = serde_json::to_string_pretty(&goal)?;
        std::fs::write(&path, content)?;

        // 更新内存缓存
        self.goals.insert(id, goal);

        // 更新索引
        self.save_index()?;

        Ok(())
    }

    /// ⭐ 更新已存在的 Goal
    pub fn update(&mut self, goal: Goal) -> anyhow::Result<()> {
        let id = goal.id.clone();
        let path = self.goal_path(&id);

        // 写入 Goal 文件
        let content = serde_json::to_string_pretty(&goal)?;
        std::fs::write(&path, content)?;

        // 更新内存缓存
        self.goals.insert(id, goal);

        // 更新索引
        self.save_index()?;

        Ok(())
    }

    /// ⭐ 根据 ID 获取 Goal
    pub fn get(&self, id: &str) -> Option<&Goal> {
        self.goals.get(id)
    }

    /// ⭐ 根据 ID 获取可变引用
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Goal> {
        self.goals.get_mut(id)
    }

    /// ⭐ 获取所有 Goal
    pub fn list(&self) -> Vec<&Goal> {
        let mut goals: Vec<&Goal> = self.goals.values().collect();
        goals.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // 最新在前
        goals
    }

    /// ⭐ 获取当前活跃的 Goal（状态为 Active 的第一个）
    pub fn active_goal(&self) -> Option<&Goal> {
        self.goals.values().find(|g| g.status == GoalStatus::Active)
    }

    /// ⭐ 获取活跃 Goal 的可变引用
    pub fn active_goal_mut(&mut self) -> Option<&mut Goal> {
        self.goals.values_mut().find(|g| g.status == GoalStatus::Active)
    }

    /// ⭐ 是否有活跃的 Goal
    pub fn has_active_goal(&self) -> bool {
        self.goals.values().any(|g| g.status == GoalStatus::Active)
    }

    /// ⭐ 删除 Goal
    pub fn delete(&mut self, id: &str) -> anyhow::Result<bool> {
        if self.goals.remove(id).is_some() {
            let path = self.goal_path(id);
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            self.save_index()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// ⭐ 根据描述创建新 Goal（便捷方法）
    pub fn create_goal(&mut self, description: String) -> anyhow::Result<Goal> {
        use super::types::Goal;
        let now = super::types::chrono_now();
        let goal = Goal::new(
            description.clone(),
            description,
            vec![],
        );
        let id = goal.id.clone();
        self.create(goal)?;
        // 自动激活 - clone first to avoid borrow conflict
        let loaded_clone = {
            let loaded = self.get_mut(&id)
                .ok_or_else(|| anyhow::anyhow!("goal not found after creation"))?;
            loaded.activate();
            loaded.updated_at = now;
            loaded.clone()
        };
        self.save_index()?;
        Ok(loaded_clone)
    }

    /// ⭐ 标记目标为完成
    pub fn mark_complete(&mut self, id: &str) -> anyhow::Result<bool> {
        // Clone the goal first to avoid borrow conflict
        let updated = if let Some(goal) = self.get_mut(id) {
            goal.complete();
            goal.updated_at = super::types::chrono_now();
            Some(goal.clone())
        } else {
            None
        };
        if let Some(goal) = updated {
            self.update(goal)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// ⭐ 标记目标为失败
    pub fn mark_failed(&mut self, id: &str, _reason: &str) -> anyhow::Result<bool> {
        let updated = if let Some(goal) = self.get_mut(id) {
            goal.fail();
            goal.updated_at = super::types::chrono_now();
            Some(goal.clone())
        } else {
            None
        };
        if let Some(goal) = updated {
            self.update(goal)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// ⭐ 标记目标为取消
    pub fn mark_cancelled(&mut self, id: &str) -> anyhow::Result<bool> {
        let updated = if let Some(goal) = self.get_mut(id) {
            goal.cancel();
            goal.updated_at = super::types::chrono_now();
            Some(goal.clone())
        } else {
            None
        };
        if let Some(goal) = updated {
            self.update(goal)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// ⭐ 获取注入消息（用于压缩后重新注入上下文）
    pub fn get_inject_message(&self) -> Option<crate::model::ChatMessage> {
        let prompt = self.get_goal_context_prompt()?;
        Some(crate::model::ChatMessage::system(&prompt))
    }

    /// 获取活跃 Goal 的上下文注入文本
    pub fn get_goal_context_prompt(&self) -> Option<String> {
        let goal = self.active_goal()?;

        let mut lines = Vec::new();
        lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
        lines.push("【🎯 目标驱动模式 — 活跃目标】".to_string());
        lines.push(String::new());
        lines.push(format!("  目标: {}", goal.name));
        if !goal.description.is_empty() {
            lines.push(format!("  描述: {}", goal.description));
        }
        lines.push(format!("  进度: {}", goal.progress_text()));
        lines.push(format!("  轮次: {}/{}", goal.turn_count, goal.max_turns));
        lines.push(String::new());

        if !goal.criteria.is_empty() {
            lines.push("  完成标准:".to_string());
            for (i, c) in goal.criteria.iter().enumerate() {
                let done = goal.completed_steps.contains(&format!("criteria_{}", i));
                let prefix = if done { "✅" } else { "⬜" };
                lines.push(format!("    {} {}", prefix, c));
            }
            lines.push(String::new());
        }

        if !goal.completed_steps.is_empty() {
            lines.push("  已完成步骤:".to_string());
            for step in &goal.completed_steps {
                lines.push(format!("    ✅ - {}", step));
            }
            lines.push(String::new());
        }

        if !goal.steps.is_empty() && goal.completed_steps.len() < goal.steps.len() {
            lines.push("  待完成步骤:".to_string());
            for step in &goal.steps {
                if !goal.completed_steps.contains(step) {
                    lines.push(format!("    ⬜ - {}", step));
                }
            }
            lines.push(String::new());
        }

        // 自评估指令
        if goal.all_steps_done() {
            lines.push("  【所有步骤已完成！请进行自评估】".to_string());
            lines.push("  请检查所有完成标准是否符合要求。".to_string());
            lines.push("  如果全部满足，在回复中输出 `/goal complete` 标记完成。".to_string());
            lines.push("  如果无法完成，输出 `/goal fail <原因>` 标记失败。".to_string());
        } else {
            lines.push("  【自评估指令】".to_string());
            lines.push("  请继续执行未完成的步骤。".to_string());
            lines.push("  如果判断目标无法完成，输出 `/goal fail <原因>`。".to_string());
            lines.push("  如果用户要求取消，输出 `/goal cancel`。".to_string());
        }

        lines.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());

        Some(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_registry_create_and_get() {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.path().to_string_lossy().to_string();

        let mut registry = GoalRegistry::new(&root);
        let goal = Goal::new(
            "测试目标".to_string(),
            "描述".to_string(),
            vec!["条件1".to_string()],
        );
        let id = goal.id.clone();

        registry.create(goal).unwrap();
        let loaded = registry.get(&id).unwrap();
        assert_eq!(loaded.name, "测试目标");
        assert_eq!(loaded.status, GoalStatus::Proposed);
    }

    #[test]
    fn test_registry_update() {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.path().to_string_lossy().to_string();

        let mut registry = GoalRegistry::new(&root);
        let mut goal = Goal::new(
            "测试".to_string(),
            "".to_string(),
            vec![],
        );
        let id = goal.id.clone();
        registry.create(goal).unwrap();

        let mut loaded = registry.get(&id).unwrap().clone();
        loaded.activate();
        registry.update(loaded).unwrap();

        let updated = registry.get(&id).unwrap();
        assert_eq!(updated.status, GoalStatus::Active);
    }

    #[test]
    fn test_registry_load_all() {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.path().to_string_lossy().to_string();

        let mut registry = GoalRegistry::new(&root);
        let g1 = Goal::new("目标1".to_string(), "".to_string(), vec![]);
        let g2 = Goal::new("目标2".to_string(), "".to_string(), vec![]);
        let id1 = g1.id.clone();
        let id2 = g2.id.clone();

        registry.create(g1).unwrap();
        registry.create(g2).unwrap();

        // 新建一个 registry 并加载
        let mut registry2 = GoalRegistry::new(&root);
        registry2.load_all().unwrap();

        assert_eq!(registry2.list().len(), 2);
        assert!(registry2.get(&id1).is_some());
        assert!(registry2.get(&id2).is_some());
    }

    #[test]
    fn test_active_goal() {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.path().to_string_lossy().to_string();

        let mut registry = GoalRegistry::new(&root);
        let mut goal = Goal::new("活跃目标".to_string(), "".to_string(), vec![]);
        let id = goal.id.clone();
        goal.activate();
        registry.create(goal).unwrap();

        assert!(registry.has_active_goal());
        let active = registry.active_goal().unwrap();
        assert_eq!(active.id, id);
    }

    #[test]
    fn test_delete() {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.path().to_string_lossy().to_string();

        let mut registry = GoalRegistry::new(&root);
        let goal = Goal::new("待删除".to_string(), "".to_string(), vec![]);
        let id = goal.id.clone();
        registry.create(goal).unwrap();
        assert_eq!(registry.list().len(), 1);

        registry.delete(&id).unwrap();
        assert_eq!(registry.list().len(), 0);
    }

    #[test]
    fn test_goal_context_prompt() {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.path().to_string_lossy().to_string();

        let mut registry = GoalRegistry::new(&root);
        let mut goal = Goal::new(
            "测试目标".to_string(),
            "测试描述".to_string(),
            vec!["条件A".to_string(), "条件B".to_string()],
        );
        goal.steps = vec!["步骤1".to_string(), "步骤2".to_string()];
        goal.activate();
        goal.record_step_completed("步骤1".to_string());
        registry.create(goal).unwrap();

        let prompt = registry.get_goal_context_prompt().unwrap();
        assert!(prompt.contains("测试目标"));
        assert!(prompt.contains("条件A"));
        assert!(prompt.contains("步骤1"));
        assert!(prompt.contains("🎯"));
    }
}
