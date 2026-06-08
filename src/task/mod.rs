// src/task/mod.rs
//
// 结构化任务执行框架 — TaskManager
//
// 核心职责：
// 1. 维护当前任务状态（内存中）
// 2. 读写 PLAN.md / AGENDA.md / MEMORY.md（持久化）
// 3. 上下文压缩后生成状态提示（注入到 LLM 上下文）
// 4. 提供 Agent 可用的"自我状态感知"能力
//
// 使用方式：
//   let mut tm = TaskManager::new("./");
//   tm.load();                             // 启动时加载
//   // ... 用户输入后 ...
//   tm.on_user_input("实现...");            // 检测新任务
//   // ... 压缩发生后 ...
//   let prompt = tm.get_inject_prompt();    // 生成恢复提示
//   tm.save();                             // 定期保存

use std::path::PathBuf;

use crate::model::ChatMessage;

pub mod types;
use types::TaskState;

/// ⭐ 任务管理器
pub struct TaskManager {
    root_dir: PathBuf,
    state: TaskState,
    is_dirty: bool,
    /// 上次注入的状态摘要 hash，避免压缩后重复注入相同内容
    last_injected_hash: u64,
}

impl TaskManager {
    /// 创建新的任务管理器，指定项目根目录
    pub fn new(root_dir: impl Into<String>) -> Self {
        Self {
            root_dir: PathBuf::from(root_dir.into()),
            state: TaskState::default(),
            is_dirty: false,
            last_injected_hash: 0,
        }
    }

    /// ⭐ 从文件加载状态
    ///
    /// 只在启动时调用一次。运行时状态由 Agent 通过工具写入文件，
    /// TaskManager 不主动解析文件内容（避免与 LLM 写入冲突），
    /// 而是通过 `on_user_input` 和 `on_step_complete` 等方法跟踪。
    pub fn load(&mut self) {
        // 读取所有状态文件，尝试恢复任务上下文
        let plan_path = self.root_dir.join("docs/PLAN.md");
        let agenda_path = self.root_dir.join("docs/AGENDA.md");
        let memory_path = self.root_dir.join("docs/MEMORY.md");

        // 简单启发式：从 AGENDA 中提取任务名
        if let Ok(content) = std::fs::read_to_string(&agenda_path) {
            for line in content.lines() {
                if line.starts_with("## 任务：") {
                    let task_name = line.trim_start_matches("## 任务：").trim();
                    if !task_name.is_empty() && self.state.current_task.is_none() {
                        self.state.current_task = Some(task_name.to_string());
                    }
                } else if line.starts_with("**当前**:") {
                    let step = line.trim_start_matches("**当前**:").trim();
                    if !step.is_empty() {
                        self.state.current_step = Some(step.to_string());
                    }
                }
            }
        }

        // 从 PLAN.md 提取步骤
        if let Ok(content) = std::fs::read_to_string(&plan_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("- [ ]") {
                    let step = trimmed.trim_start_matches("- [ ]").trim();
                    if !step.is_empty() && !self.state.pending_steps.contains(&step.to_string()) {
                        self.state.pending_steps.push(step.to_string());
                    }
                } else if trimmed.starts_with("- [x]") {
                    let step = trimmed.trim_start_matches("- [x]").trim();
                    if !step.is_empty() && !self.state.completed_steps.contains(&step.to_string()) {
                        self.state.completed_steps.push(step.to_string());
                    }
                }
            }
        }

        // 从 MEMORY.md 提取重要发现
        if let Ok(content) = std::fs::read_to_string(&memory_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("- 【") && trimmed.contains('】') {
                    let fact = trimmed.trim_start_matches("- ").to_string();
                    if !self.state.important_facts.contains(&fact) {
                        self.state.important_facts.push(fact);
                    }
                }
            }
        }

        self.is_dirty = false;
    }

    /// ⭐ 保存状态到文件
    ///
    /// 将内存中的状态同步到 PLAN.md, AGENDA.md, MEMORY.md。
    /// 仅在状态有变更时（is_dirty=true）才写入，减少磁盘 IO。
    pub fn save(&self) -> anyhow::Result<()> {
        if !self.is_dirty {
            return Ok(());
        }

        let plan_path = self.root_dir.join("docs/PLAN.md");
        let agenda_path = self.root_dir.join("docs/AGENDA.md");
        let _memory_path = self.root_dir.join("docs/MEMORY.md");

        // 写入 AGENDA.md（精简的任务当前进度）
        let agenda_content = self.format_agenda();
        std::fs::write(&agenda_path, agenda_content)?;

        // 写入 PLAN.md（完整步骤列表）
        let plan_content = self.format_plan();
        std::fs::write(&plan_path, plan_content)?;

        Ok(())
    }

    /// 生成 AGENDA.md 内容
    fn format_agenda(&self) -> String {
        let mut lines = Vec::new();
        lines.push("# 当前议程".to_string());
        lines.push(String::new());

        if let Some(task) = &self.state.current_task {
            lines.push(format!("## 任务：{}", task));
            let total = self.state.completed_steps.len() + self.state.pending_steps.len();
            if total > 0 {
                lines.push(format!(
                    "**进度**: {}/{} 步完成 | **当前**: {}",
                    self.state.completed_steps.len(),
                    total,
                    self.state.current_step.as_deref().unwrap_or("规划中"),
                ));
            }
            lines.push(String::new());
        } else {
            lines.push("*暂无活跃任务*".to_string());
            lines.push(String::new());
        }

        if !self.state.completed_steps.is_empty() {
            lines.push("### ✅ 已完成".to_string());
            for step in &self.state.completed_steps {
                lines.push(format!("- [x] {}", step));
            }
            lines.push(String::new());
        }

        if !self.state.pending_steps.is_empty() {
            lines.push("### ⏳ 待完成".to_string());
            for step in &self.state.pending_steps {
                lines.push(format!("- [ ] {}", step));
            }
            lines.push(String::new());
        }

        if !self.state.important_facts.is_empty() {
            lines.push("### 🧠 重要发现".to_string());
            for fact in &self.state.important_facts {
                lines.push(format!("- {}", fact));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    }

    /// 生成 PLAN.md 内容
    fn format_plan(&self) -> String {
        let mut lines = Vec::new();

        if let Some(task) = &self.state.current_task {
            lines.push(format!("# {}", task));
        } else {
            lines.push("# 执行计划".to_string());
        }
        lines.push(String::new());

        lines.push("## 执行步骤".to_string());
        lines.push(String::new());

        for step in &self.state.completed_steps {
            lines.push(format!("- [x] {}", step));
        }
        for step in &self.state.pending_steps {
            lines.push(format!("- [ ] {}", step));
        }

        if !self.state.important_facts.is_empty() {
            lines.push(String::new());
            lines.push("## 重要发现".to_string());
            lines.push(String::new());
            for fact in &self.state.important_facts {
                lines.push(format!("- {}", fact));
            }
        }

        lines.push(String::new());
        lines.push("---".to_string());
        lines.push(format!("_最后更新: {}_", self.state.last_updated));

        lines.join("\n")
    }

    /// ⭐ 获取需要注入到上下文的提示
    ///
    /// 返回一个 ChatMessage::System 消息，包含当前任务状态。
    /// 如果状态没有变化（hash 相同），返回 None 避免重复注入。
    pub fn get_inject_message(&mut self) -> Option<ChatMessage> {
        if self.state.is_idle() {
            return None;
        }

        let prompt = self.state.to_context_prompt();
        let hash = simple_hash(&prompt);

        if hash == self.last_injected_hash {
            return None;
        }

        self.last_injected_hash = hash;
        Some(ChatMessage::system(prompt))
    }

    /// ⭐ 检测新任务开始
    ///
    /// 当用户输入新内容时调用。如果当前处于空闲状态，
    /// 且输入看起来像是一个新任务，自动初始化任务状态。
    pub fn on_user_input(&mut self, input: &str) {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
        }

        // 如果当前空闲，将用户输入视为新任务
        if self.state.is_idle() && trimmed.len() > 3 {
            // 简短输入（如"继续"、"yes"）不视为新任务
            self.state.current_task = Some(trimmed.to_string());
            self.state.pending_steps.clear();
            self.state.completed_steps.clear();
            self.state.current_step = None;
            self.mark_dirty();
        }
    }

    /// 标记状态已变更，下次 save() 会写入文件
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
        self.state.last_updated = types::chrono_now();
    }

    /// 获取当前状态的只读引用
    pub fn state(&self) -> &TaskState {
        &self.state
    }

    /// 获取可变状态引用（修改后需调用 mark_dirty）
    pub fn state_mut(&mut self) -> &mut TaskState {
        &mut self.state
    }
}

/// 简单字符串哈希（用于去重检测）
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for b in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 创建临时目录用于测试
    fn setup_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("创建临时目录失败")
    }

    #[test]
    fn test_task_manager_new() {
        let tm = TaskManager::new("./");
        assert!(tm.state.is_idle());
        assert!(!tm.is_dirty);
    }

    #[test]
    fn test_on_user_input_starts_task() {
        let mut tm = TaskManager::new("./");
        assert!(tm.state.is_idle());

        tm.on_user_input("帮我重构用户模块");
        assert_eq!(tm.state.current_task.as_deref(), Some("帮我重构用户模块"));
        assert!(tm.is_dirty);
    }

    #[test]
    fn test_on_user_input_short_ignored() {
        let mut tm = TaskManager::new("./");
        tm.on_user_input("hi");
        assert!(tm.state.is_idle());
    }

    #[test]
    fn test_get_inject_message_idle() {
        let mut tm = TaskManager::new("./");
        assert!(tm.get_inject_message().is_none());
    }

    #[test]
    fn test_get_inject_message_dedup() {
        let mut tm = TaskManager::new("./");
        tm.state.current_task = Some("测试任务".to_string());
        tm.state.pending_steps.push("步骤1".to_string());

        let msg1 = tm.get_inject_message();
        assert!(msg1.is_some());

        let msg2 = tm.get_inject_message();
        assert!(msg2.is_none()); // 相同内容，去重
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = setup_temp_dir();
        let root = dir.path().to_str().unwrap().to_string();

        // 创建必要的目录
        std::fs::create_dir_all(Path::new(&root).join("docs")).unwrap();

        let mut tm = TaskManager::new(&root);
        tm.state.current_task = Some("集成测试".to_string());
        tm.state.completed_steps.push("分析".to_string());
        tm.state.pending_steps.push("编码".to_string());
        tm.state.pending_steps.push("测试".to_string());
        tm.state
            .important_facts
            .push("需要处理边界情况".to_string());
        tm.mark_dirty();
        tm.save().unwrap();

        // 验证文件存在
        assert!(Path::new(&root).join("docs/PLAN.md").exists());
        assert!(Path::new(&root).join("docs/AGENDA.md").exists());

        // 读取文件内容验证
        let agenda = std::fs::read_to_string(Path::new(&root).join("docs/AGENDA.md")).unwrap();
        assert!(agenda.contains("集成测试"));
        assert!(agenda.contains("分析"));
        assert!(agenda.contains("编码"));
        assert!(agenda.contains("需要处理边界情况"));

        let plan = std::fs::read_to_string(Path::new(&root).join("docs/PLAN.md")).unwrap();
        assert!(plan.contains("集成测试"));
        assert!(plan.contains("- [x] 分析"));
        assert!(plan.contains("- [ ] 编码"));
    }

    #[test]
    fn test_load_from_files() {
        let dir = setup_temp_dir();
        let root = dir.path().to_str().unwrap().to_string();

        // 创建目录和文件
        std::fs::create_dir_all(Path::new(&root).join("docs")).unwrap();

        // 写入 AGENDA.md
        let agenda_content = r#"# 当前议程

## 任务：数据库迁移
**进度**: 1/3 步完成 | **当前**: 编写迁移脚本

### ✅ 已完成
- [x] 分析表结构

### ⏳ 待完成
- [ ] 编写迁移脚本
- [ ] 测试验证
"#;
        std::fs::write(Path::new(&root).join("docs/AGENDA.md"), agenda_content).unwrap();

        // 写入 PLAN.md
        let plan_content = r#"# 数据库迁移

## 执行步骤

- [x] 分析表结构
- [ ] 编写迁移脚本
- [ ] 测试验证

## 重要发现
- 用户表有外键约束

_最后更新: 2024-06-07_
"#;
        std::fs::write(Path::new(&root).join("docs/PLAN.md"), plan_content).unwrap();

        // 写入 MEMORY.md
        let memory_content = r#"# MEMORY.md

## 重要发现
- 【决策】使用异步迁移方式
- 【发现】用户表有外键约束
"#;
        std::fs::write(Path::new(&root).join("docs/MEMORY.md"), memory_content).unwrap();

        // 加载
        let mut tm = TaskManager::new(&root);
        tm.load();

        assert_eq!(tm.state.current_task.as_deref(), Some("数据库迁移"));
        assert!(tm.state.completed_steps.contains(&"分析表结构".to_string()));
        assert!(tm.state.pending_steps.contains(&"编写迁移脚本".to_string()));
        assert!(
            tm.state
                .important_facts
                .iter()
                .any(|f| f.contains("外键约束"))
        );
    }
}
