// src/session/mod.rs
//
// 会话管理模块（SessionManager）
//
// 核心职责：
// 1. 保存/加载对话上下文（ContextManager 的消息列表）到文件
// 2. 列出、删除、重命名已保存的会话
// 3. 提供 CLI 命令的处理接口
//
// 会话文件存储在 `.sessions/` 目录下，格式为 JSON。
// 文件命名规则：`<会话名称>.session.json`
//
// 用例：
//   /session save my-work   → 保存当前对话为 "my-work"
//   /session load my-work   → 加载 "my-work" 会话
//   /session list           → 列出所有会话
//   /session delete my-work → 删除 "my-work" 会话
//   /session rename old new → 将会话 "old" 重命名为 "new"

use std::path::PathBuf;

use self::types::*;
use crate::context::ContextManager;
use crate::model::ChatMessage;

/// ⭐ 会话管理器
pub struct SessionManager {
    /// 会话存储目录
    sessions_dir: PathBuf,
    /// 当前工作目录（创建会话时记录）
    current_dir: String,
}

impl SessionManager {
    /// 创建新的会话管理器
    ///
    /// `root_dir`: 项目根目录（.sessions/ 创建在此目录下）
    /// `current_dir`: 当前工作目录（用于保存到会话元数据）
    pub fn new(root_dir: impl Into<String>, current_dir: impl Into<String>) -> Self {
        let root = PathBuf::from(root_dir.into());
        let sessions_dir = root.join(".sessions");
        Self {
            sessions_dir,
            current_dir: current_dir.into(),
        }
    }

    /// 确保 sessions 目录存在
    pub fn ensure_dir(&self) -> anyhow::Result<()> {
        if !self.sessions_dir.exists() {
            std::fs::create_dir_all(&self.sessions_dir)?;
        }
        Ok(())
    }

    /// 获取会话文件路径
    fn session_path(&self, name: &str) -> PathBuf {
        let safe_name = sanitize_name(name);
        self.sessions_dir
            .join(format!("{}.session.json", safe_name))
    }

    /// ⭐ 保存当前上下文为会话
    ///
    /// 从 ContextManager 中提取所有消息，保存到文件。
    /// 如果同名会话已存在，会覆盖（更新 updated_at）。
    pub fn save(&self, name: &str, ctx: &ContextManager) -> anyhow::Result<SessionData> {
        self.ensure_dir()?;

        let path = self.session_path(name);
        let now = crate::task::types::chrono_now();

        // 从 ContextManager 中提取消息（跳过系统提示词，系统提示词会重建）
        let messages: Vec<SerializedContextMessage> = ctx
            .get_messages()
            .iter()
            .filter(|m| {
                // 过滤掉系统提示词（由 strategy 重建）
                !matches!(m, ChatMessage::System { .. })
            })
            .map(|msg| SerializedContextMessage {
                message: SerializableMessage::from(msg),
                preserved: false,
            })
            .collect();

        // 获取策略
        let strategy = ctx.strategy().clone();

        let session = SessionData {
            name: name.to_string(),
            created_at: if path.exists() {
                // 如果已存在，读取原始的 created_at
                let existing = std::fs::read_to_string(&path)?;
                if let Ok(data) = serde_json::from_str::<SessionData>(&existing) {
                    data.created_at
                } else {
                    now.clone()
                }
            } else {
                now.clone()
            },
            updated_at: now,
            messages,
            strategy,
            current_dir: self.current_dir.clone(),
            version: 1,
        };

        let json = serde_json::to_string_pretty(&session)?;
        std::fs::write(&path, json)?;

        Ok(session)
    }

    /// ⭐ 保存带 preserved 标志的会话（更精确的版本）
    ///
    /// 需要访问 ContextManager 的内部消息列表（Vec<ContextMessage>）。
    pub fn save_with_preserved(
        &self,
        name: &str,
        ctx: &ContextManager,
        messages: &[crate::context::ContextMessage],
    ) -> anyhow::Result<SessionData> {
        self.ensure_dir()?;

        let path = self.session_path(name);
        let now = crate::task::types::chrono_now();

        // 提取消息（跳过系统提示词）
        let serialized_messages: Vec<SerializedContextMessage> = messages
            .iter()
            .filter(|cm| !matches!(cm.message, ChatMessage::System { .. }))
            .map(|cm| SerializedContextMessage {
                message: SerializableMessage::from(&cm.message),
                preserved: cm.preserved,
            })
            .collect();

        let strategy = ctx.strategy().clone();

        let session = SessionData {
            name: name.to_string(),
            created_at: if path.exists() {
                let existing = std::fs::read_to_string(&path)?;
                if let Ok(data) = serde_json::from_str::<SessionData>(&existing) {
                    data.created_at
                } else {
                    now.clone()
                }
            } else {
                now.clone()
            },
            updated_at: now,
            messages: serialized_messages,
            strategy,
            current_dir: self.current_dir.clone(),
            version: 1,
        };

        let json = serde_json::to_string_pretty(&session)?;
        std::fs::write(&path, json)?;

        Ok(session)
    }

    /// ⭐ 加载会话
    ///
    /// 从文件加载会话数据，返回 SessionData。
    pub fn load(&self, name: &str) -> anyhow::Result<SessionData> {
        let path = self.session_path(name);
        let json = std::fs::read_to_string(&path)?;
        let session: SessionData = serde_json::from_str(&json)?;
        Ok(session)
    }

    /// ⭐ 从 SessionData 重建消息列表
    ///
    /// 返回消息列表（含系统提示词）
    pub fn restore_messages(&self, session: &SessionData, system_prompt: &str) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // 添加系统提示词
        messages.push(ChatMessage::system(system_prompt));

        // 恢复用户/助手/工具消息
        for stored_msg in &session.messages {
            let msg: ChatMessage = stored_msg.message.clone().into();
            messages.push(msg);
        }

        messages
    }

    /// 获取默认的系统提示词（从 session 的策略中重建）
    pub fn default_system_prompt(&self, session: &SessionData) -> String {
        format!(
            "你正在从已保存会话恢复 Agent Lab 对话。\n\n\
            工作目录: {}\n\
            会话名称: {}\n\
            会话创建于: {}\n\
            最后更新于: {}\n\n\
            [恢复原则]\n\
            - 先结合恢复的消息、用户最新输入、任务文件和长期记忆判断当前目标，不要假设旧计划仍然有效。\n\
            - Agent Lab 是一个 Rust 编写的自我进化 Agent 框架；核心模块包括 agent 主循环、模型适配、工具系统、上下文压缩、Task/Goal/Session、长期记忆和 Swarm 多 Agent 编排。\n\
            - 简单问题直接回答；明确的实现、修复、排查任务应主动执行到可验证状态。\n\
            - 多步任务按「理解目标 -> 调查现状 -> 制定短计划 -> 执行 -> 验证 -> 总结」推进。\n\
            - 优先读取本地代码和文档，改动保持聚焦，不覆盖用户已有修改，不执行破坏性仓库操作。\n\
            - TaskManager 的结构化状态入口是 docs/PLAN.md、docs/AGENDA.md、docs/MEMORY.md；根目录同名文件可能保存历史或人工记录，冲突时以当前用户目标、代码事实和最近状态为准。\n\
            - 修改 Rust 代码后运行 cargo check；涉及共享行为、上下文、Goal、Session、Memory、工具协议或 Swarm 时优先运行相关 cargo test。\n\
            - 可以使用 memory_search/memory_save 管理长期记忆，使用 spawn_agent 进行独立验证或并行调查，使用 investigate 分析错误快照，使用 generate_tool 扩展新工具能力。\n\
            - 有活跃 Goal 时主动推进并验证；确认完成后输出 /goal complete <目标ID>，无法完成时输出 /goal fail <目标ID> <原因>。\n\
            - 最终回复用用户语言，说明完成内容、关键文件、验证结果和剩余风险。",
            session.current_dir, session.name, session.created_at, session.updated_at,
        )
    }

    /// ⭐ 列出所有会话
    pub fn list(&self) -> anyhow::Result<Vec<SessionInfo>> {
        self.ensure_dir()?;

        let mut sessions = Vec::new();

        if !self.sessions_dir.exists() {
            return Ok(sessions);
        }

        for entry in std::fs::read_dir(&self.sessions_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |n| n.ends_with(".session.json"))
            {
                if let Ok(json) = std::fs::read_to_string(&path) {
                    if let Ok(data) = serde_json::from_str::<SessionData>(&json) {
                        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        sessions.push(SessionInfo {
                            name: data.name,
                            created_at: data.created_at,
                            updated_at: data.updated_at,
                            message_count: data.messages.len(),
                            file_size,
                        });
                    } else {
                        // 如果解析失败，尝试从文件名提取名称
                        if let Some(name) = path
                            .file_stem()
                            .and_then(|n| n.to_str())
                            .map(|n| n.trim_end_matches(".session"))
                        {
                            let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            sessions.push(SessionInfo {
                                name: name.to_string(),
                                created_at: "未知".to_string(),
                                updated_at: "未知".to_string(),
                                message_count: 0,
                                file_size,
                            });
                        }
                    }
                }
            }
        }

        // 按更新时间排序（最新的在前）
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(sessions)
    }

    /// 删除会话
    pub fn delete(&self, name: &str) -> anyhow::Result<bool> {
        let path = self.session_path(name);
        if path.exists() {
            std::fs::remove_file(&path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 重命名会话
    pub fn rename(&self, old_name: &str, new_name: &str) -> anyhow::Result<bool> {
        let old_path = self.session_path(old_name);
        let new_path = self.session_path(new_name);

        if !old_path.exists() {
            return Ok(false);
        }

        if new_path.exists() {
            return Err(anyhow::anyhow!("目标名称 '{}' 已存在", new_name));
        }

        // 读取原文件，更新 name 字段
        let json = std::fs::read_to_string(&old_path)?;
        let mut session: SessionData = serde_json::from_str(&json)?;
        session.name = new_name.to_string();
        session.updated_at = crate::task::types::chrono_now();

        let new_json = serde_json::to_string_pretty(&session)?;
        std::fs::write(&new_path, new_json)?;
        std::fs::remove_file(&old_path)?;

        Ok(true)
    }

    /// 获取会话数量
    pub fn count(&self) -> anyhow::Result<usize> {
        Ok(self.list()?.len())
    }
}

/// 会话摘要信息（用于列表展示）
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
    pub file_size: u64,
}

/// 净化文件名（去除特殊字符）
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else if c == ' ' {
                '_' // 空格替换为下划线（可读性）
            } else {
                ' ' // 其他特殊字符移除（后续通过过滤空格实现）
            }
        })
        .filter(|c| *c != ' ')
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

impl std::fmt::Display for SessionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size_str = if self.file_size > 1024 * 1024 {
            format!("{:.1} MB", self.file_size as f64 / (1024.0 * 1024.0))
        } else if self.file_size > 1024 {
            format!("{:.1} KB", self.file_size as f64 / 1024.0)
        } else {
            format!("{} B", self.file_size)
        };

        write!(
            f,
            "  📁 {:<20} 消息: {:<4} 大小: {:<8} 更新: {}",
            self.name, self.message_count, size_str, self.updated_at,
        )
    }
}

pub mod types;

#[cfg(test)]
mod tests;
