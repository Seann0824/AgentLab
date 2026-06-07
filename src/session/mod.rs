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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::context::{ContextManager, ContextStrategy};
use crate::model::{ChatMessage, ToolCall};

/// ⭐ 可序列化的消息类型（用于持久化）
///
/// ChatMessage 包含工具调用等复杂结构，直接序列化可能丢失信息。
/// 这里定义一个显式的、自描述的格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum SerializableMessage {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant {
        content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<SerializableToolCall>,
    },
    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// 可序列化的工具调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl From<&ChatMessage> for SerializableMessage {
    fn from(msg: &ChatMessage) -> Self {
        match msg {
            ChatMessage::System { content } => {
                SerializableMessage::System {
                    content: content.clone(),
                }
            }
            ChatMessage::User { content } => {
                SerializableMessage::User {
                    content: content.clone(),
                }
            }
            ChatMessage::Assistant { content, tool_calls } => {
                SerializableMessage::Assistant {
                    content: content.clone(),
                    tool_calls: tool_calls.iter().map(|tc| SerializableToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    }).collect(),
                }
            }
            ChatMessage::Tool { tool_call_id, content } => {
                SerializableMessage::Tool {
                    tool_call_id: tool_call_id.clone(),
                    content: content.clone(),
                }
            }
        }
    }
}

impl From<SerializableMessage> for ChatMessage {
    fn from(msg: SerializableMessage) -> Self {
        match msg {
            SerializableMessage::System { content } => {
                ChatMessage::system(content)
            }
            SerializableMessage::User { content } => {
                ChatMessage::user(content)
            }
            SerializableMessage::Assistant { content, tool_calls } => {
                let tcs: Vec<ToolCall> = tool_calls.into_iter().map(|tc| ToolCall {
                    id: tc.id,
                    name: tc.name,
                    arguments: tc.arguments,
                }).collect();
                if tcs.is_empty() {
                    ChatMessage::assistant(content)
                } else {
                    ChatMessage::assistant_tool_calls(content, tcs)
                }
            }
            SerializableMessage::Tool { tool_call_id, content } => {
                ChatMessage::tool(tool_call_id, content)
            }
        }
    }
}

/// ⭐ 会话数据（完整的可持久化状态）
///
/// 包含：
/// - 元数据（名称、创建/更新时间）
/// - 消息列表（所有对话消息 + preserved 标记）
/// - 上下文策略（用于重建 ContextManager）
/// - 额外的元信息（如当前工作目录等）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// 会话名称
    pub name: String,
    /// 创建时间
    pub created_at: String,
    /// 最后修改时间
    pub updated_at: String,
    /// 消息列表（带 preserved 标记）
    pub messages: Vec<SerializedContextMessage>,
    /// 压缩策略
    pub strategy: ContextStrategy,
    /// 当前工作目录（用于恢复上下文）
    pub current_dir: String,
    /// 版本（便于未来升级迁移）
    pub version: u32,
}

/// 带 preserved 标记的可序列化消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedContextMessage {
    /// 消息内容
    pub message: SerializableMessage,
    /// 是否标记为永久保留
    #[serde(default)]
    pub preserved: bool,
}

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
        self.sessions_dir.join(format!("{}.session.json", safe_name))
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
        let messages: Vec<SerializedContextMessage> = ctx.get_messages()
            .iter()
            .filter(|m| {
                // 过滤掉系统提示词（由 strategy 重建）
                !matches!(m, ChatMessage::System { .. })
            })
            .map(|msg| {
                SerializedContextMessage {
                    message: SerializableMessage::from(msg),
                    preserved: false,
                }
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
            .filter(|cm| {
                !matches!(cm.message, ChatMessage::System { .. })
            })
            .map(|cm| {
                SerializedContextMessage {
                    message: SerializableMessage::from(&cm.message),
                    preserved: cm.preserved,
                }
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
            "你当前工作的目录为 {}。这个目录是你模型的Agent架子，它构建你和外部世界沟通的 bridge。如果你需要什么能力自己修改agent代码补充。\n\n\
            这是从上次保存的会话恢复的对话。继续之前的工作。\n\
            会话名称: {}\n\
            会话创建于: {}\n\
            最后更新于: {}",
            session.current_dir,
            session.name,
            session.created_at,
            session.updated_at,
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
                && path.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |n| n.ends_with(".session.json"))
            {
                if let Ok(json) = std::fs::read_to_string(&path) {
                    if let Ok(data) = serde_json::from_str::<SessionData>(&json) {
                        let file_size = std::fs::metadata(&path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        sessions.push(SessionInfo {
                            name: data.name,
                            created_at: data.created_at,
                            updated_at: data.updated_at,
                            message_count: data.messages.len(),
                            file_size,
                        });
                    } else {
                        // 如果解析失败，尝试从文件名提取名称
                        if let Some(name) = path.file_stem()
                            .and_then(|n| n.to_str())
                            .map(|n| n.trim_end_matches(".session"))
                        {
                            let file_size = std::fs::metadata(&path)
                                .map(|m| m.len())
                                .unwrap_or(0);
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
            self.name,
            self.message_count,
            size_str,
            self.updated_at,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextManager;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("my-session"), "my-session");
        assert_eq!(sanitize_name("hello world"), "hello_world");
        assert_eq!(sanitize_name("test/session"), "testsession");
        assert_eq!(sanitize_name("___"), "");
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let original = vec![
            SerializableMessage::System { content: "system prompt".to_string() },
            SerializableMessage::User { content: "hello".to_string() },
            SerializableMessage::Assistant {
                content: "hi there".to_string(),
                tool_calls: vec![
                    SerializableToolCall {
                        id: "call_1".to_string(),
                        name: "shell".to_string(),
                        arguments: r#"{"command": "ls"}"#.to_string(),
                    },
                ],
            },
            SerializableMessage::Tool {
                tool_call_id: "call_1".to_string(),
                content: r#"{"ok": true}"#.to_string(),
            },
        ];

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&original).unwrap();
        println!("JSON:\n{}", json);

        // Deserialize back
        let deserialized: Vec<SerializableMessage> = serde_json::from_str(&json).unwrap();
        assert_eq!(original.len(), deserialized.len());

        // Convert to ChatMessage and back
        for (orig, deser) in original.iter().zip(deserialized.iter()) {
            let chat_msg_orig: ChatMessage = orig.clone().into();
            let chat_msg_deser: ChatMessage = deser.clone().into();
            match (&chat_msg_orig, &chat_msg_deser) {
                (ChatMessage::User { content: a }, ChatMessage::User { content: b }) => {
                    assert_eq!(a, b);
                }
                (ChatMessage::Assistant { content: a, tool_calls: tc_a },
                 ChatMessage::Assistant { content: b, tool_calls: tc_b }) => {
                    assert_eq!(a, b);
                    assert_eq!(tc_a.len(), tc_b.len());
                    if !tc_a.is_empty() {
                        assert_eq!(tc_a[0].id, tc_b[0].id);
                        assert_eq!(tc_a[0].name, tc_b[0].name);
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_session_manager_save_load_roundtrip() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let root = dir.path().to_str().unwrap();

        // 创建 ContextManager
        let strategy = ContextStrategy::Auto {
            token_limit: 128_000,
            max_turns: 20,
            trigger_ratio: 0.7,
            enable_async_summary: true,
            enable_tool_pruning: true,
            tool_pruning_keep_recent: 3,
            tool_pruning_max_output_chars: 200,
        };
        let mut ctx = ContextManager::new("系统提示词", strategy);

        // 添加一些消息
        ctx.add_message(ChatMessage::user("你好"));
        ctx.add_message(ChatMessage::assistant("你好！有什么可以帮助你的吗？"));

        // 创建 SessionManager
        let sm = SessionManager::new(root, "/test/dir");

        // 保存会话
        let session = sm.save("test-session", &ctx).unwrap();
        assert_eq!(session.name, "test-session");
        assert_eq!(session.messages.len(), 2); // 跳过系统提示词
        assert_eq!(session.current_dir, "/test/dir");

        // 列出会话
        let list = sm.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-session");
        assert_eq!(list[0].message_count, 2);

        // 加载会话
        let loaded = sm.load("test-session").unwrap();
        assert_eq!(loaded.name, "test-session");
        assert_eq!(loaded.messages.len(), 2);

        // 恢复消息
        let restored = sm.restore_messages(&loaded, "新的系统提示词");
        assert_eq!(restored.len(), 3); // 系统提示词 + 2 条消息
        assert!(matches!(restored[0], ChatMessage::System { .. }));
        assert!(matches!(restored[1], ChatMessage::User { .. }));
        assert!(matches!(restored[2], ChatMessage::Assistant { .. }));

        // 删除会话
        let deleted = sm.delete("test-session").unwrap();
        assert!(deleted);
        assert_eq!(sm.list().unwrap().len(), 0);
    }
}
