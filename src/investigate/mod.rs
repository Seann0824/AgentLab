/// 错误排查 — ErrorSnapshotManager
///
/// 管理错误快照的捕获、保存、加载、列表功能。
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::ChatMessage;

pub mod types;
pub use types::*;

/// 错误快照管理器
pub struct ErrorSnapshotManager {
    /// 根目录（.agent/snapshots/ 的父目录）
    root_dir: PathBuf,
}

impl ErrorSnapshotManager {
    /// 创建管理器，指定项目根目录
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// 获取快照存储目录
    fn storage_dir(&self) -> PathBuf {
        self.root_dir.join(".agent").join("snapshots")
    }

    /// ⭐ 捕获错误现场快照
    pub fn capture(
        &self,
        ctx: &[ChatMessage],
        error_tool_name: &str,
        error_args: &serde_json::Value,
        error_output: &str,
        exit_code: Option<i32>,
        duration_ms: u64,
    ) -> ErrorSnapshot {
        // 生成快照 ID（时间戳）
        let id = format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        // 提取关键上下文（最后 6 条消息）
        let context: Vec<ChatMessage> = if ctx.len() > 6 {
            ctx[ctx.len() - 6..].to_vec()
        } else {
            ctx.to_vec()
        };

        // 将 ChatMessage 转为可序列化的形式
        let serialized_context: Vec<crate::session::types::SerializableMessage> = context
            .iter()
            .map(crate::session::types::SerializableMessage::from)
            .collect();

        // 读取任务状态文件快照
        let plan_content = Self::read_file_if_exists(&self.root_dir.join("docs/PLAN.md"));
        let agenda_content = Self::read_file_if_exists(&self.root_dir.join("docs/AGENDA.md"));

        // 构造任务上下文快照
        let task_context = TaskContextSnapshot {
            plan: plan_content,
            agenda: agenda_content,
            turn: 0,
            total_messages: ctx.len(),
        };

        // 限制错误输出长度
        let truncated_output = if error_output.len() > 2000 {
            format!(
                "{}...\n[输出截断，共 {} 字符]",
                &error_output[..2000],
                error_output.len()
            )
        } else {
            error_output.to_string()
        };

        ErrorSnapshot {
            id: id.clone(),
            created_at: format_iso_now(),
            error: ErrorInfo {
                tool_name: error_tool_name.to_string(),
                args: error_args.clone(),
                output: truncated_output,
                exit_code,
                duration_ms,
            },
            context: serialized_context,
            task_context,
        }
    }

    /// 保存快照到文件
    pub fn save(&self, snapshot: &ErrorSnapshot) -> anyhow::Result<PathBuf> {
        let storage = self.storage_dir();
        std::fs::create_dir_all(&storage)?;

        let path = storage.join(format!("{}.json", &snapshot.id));
        let data = serde_json::to_string_pretty(snapshot)?;
        std::fs::write(&path, &data)?;

        // 更新索引
        self.update_index(snapshot)?;

        Ok(path)
    }

    /// 加载快照
    pub fn load(&self, id: &str) -> anyhow::Result<ErrorSnapshot> {
        let path = self.storage_dir().join(format!("{}.json", id));
        let data = std::fs::read_to_string(&path)?;
        let snapshot: ErrorSnapshot = serde_json::from_str(&data)?;
        Ok(snapshot)
    }

    /// 列出所有快照摘要
    pub fn list(&self) -> anyhow::Result<Vec<SnapshotInfo>> {
        let index_path = self.storage_dir().join("index.json");

        if !index_path.exists() {
            return Ok(Vec::new());
        }

        let data = std::fs::read_to_string(&index_path)?;
        let list: Vec<SnapshotInfo> = serde_json::from_str(&data)?;
        Ok(list)
    }

    /// 更新索引文件（在保存新快照后）
    fn update_index(&self, snapshot: &ErrorSnapshot) -> anyhow::Result<()> {
        let index_path = self.storage_dir().join("index.json");

        let mut list = if index_path.exists() {
            let data = std::fs::read_to_string(&index_path)?;
            serde_json::from_str::<Vec<SnapshotInfo>>(&data).unwrap_or_default()
        } else {
            Vec::new()
        };

        let preview = snapshot
            .error
            .output
            .chars()
            .take(80)
            .collect::<String>()
            .replace('\n', " ");

        list.push(SnapshotInfo {
            id: snapshot.id.clone(),
            created_at: snapshot.created_at.clone(),
            tool_name: snapshot.error.tool_name.clone(),
            error_preview: preview,
        });

        // 只保留最近 50 条
        if list.len() > 50 {
            let kept = list.split_off(list.len() - 50);
            list = kept;
        }

        let data = serde_json::to_string_pretty(&list)?;
        std::fs::write(&index_path, &data)?;

        Ok(())
    }

    /// 辅助：读取文件内容（如果存在）
    fn read_file_if_exists(path: &Path) -> Option<String> {
        if path.exists() {
            std::fs::read_to_string(path).ok().map(|s| {
                s.chars().take(500).collect::<String>() // 限制长度
            })
        } else {
            None
        }
    }
}

/// ISO 格式时间
fn format_iso_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // 简单格式：2025-06-08T10:30:00
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // 从 2025-01-01 开始计算日期（简化）
    let year = 2025u64;
    let month = 1u64;
    let day = 1u64 + days;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}
