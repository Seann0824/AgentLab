# 错误排查（Error Investigation）能力 — 技术方案 — 错误快照组件

> 原文拆分自 `../replay-capability.md`。

## 3. 组件一：错误快照（ErrorSnapshot）

### 3.1 数据结构

```rust
/// 错误快照 — 工具调用报错时自动保存的「错误现场」
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSnapshot {
    /// 快照 ID（时间戳）
    pub id: String,
    /// 创建时间
    pub created_at: String,
    /// 错误信息
    pub error: ErrorInfo,
    /// 上下文消息（关键的最后几轮，不是全量）
    pub context: Vec<SerializableMessage>,
    /// 当时的任务状态（PLAN.md / AGENDA.md 内容）
    pub task_context: TaskContextSnapshot,
}

/// 错误信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// 出错的工具名称
    pub tool_name: String,
    /// 工具调用参数
    pub args: serde_json::Value,
    /// 错误输出（stdout + stderr）
    pub output: String,
    /// 退出码（如果是 shell 命令）
    pub exit_code: Option<i32>,
    /// 执行耗时（ms）
    pub duration_ms: u64,
}

/// 任务上下文快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContextSnapshot {
    /// PLAN.md 内容（如果有）
    pub plan: Option<String>,
    /// AGENDA.md 内容（如果有）
    pub agenda: Option<String>,
    /// 当前轮次
    pub turn: usize,
    /// 总消息数
    pub total_messages: usize,
}
```

### 3.2 ErrorSnapshotManager

```rust
/// 错误快照管理器
pub struct ErrorSnapshotManager {
    /// 存储目录
    storage_dir: PathBuf,
}

impl ErrorSnapshotManager {
    /// 创建管理器
    pub fn new(root_dir: &Path) -> Self;

    /// 捕获错误快照
    pub fn capture(
        &self,
        ctx: &ContextManager,
        task_manager: &TaskManager,
        error_tool_name: &str,
        error_args: &serde_json::Value,
        error_output: &str,
        exit_code: Option<i32>,
        duration_ms: u64,
    ) -> anyhow::Result<ErrorSnapshot>;

    /// 保存快照
    pub fn save(&self, snapshot: &ErrorSnapshot) -> anyhow::Result<PathBuf>;

    /// 加载快照
    pub fn load(&self, id: &str) -> anyhow::Result<ErrorSnapshot>;

    /// 列出所有快照
    pub fn list(&self) -> anyhow::Result<Vec<SnapshotInfo>>;
}

/// 快照摘要（用于列表展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub id: String,
    pub created_at: String,
    pub tool_name: String,
    pub error_preview: String,
}
```

### 3.3 存储格式

```
.agent/
├── snapshots/
│   ├── index.json                    # 索引（id → 摘要映射）
│   ├── 20250608_103000_snapshot.json # 错误快照
│   └── 20250608_104500_snapshot.json
```

**index.json 格式：**
```json
[
  {
    "id": "20250608_103000",
    "created_at": "2025-06-08T10:30:00",
    "tool_name": "shell",
    "error_preview": "cargo build 编译失败: error[E0308] type mismatch"
  }
]
```

**快照文件格式：**
```json
{
  "id": "20250608_103000",
  "created_at": "2025-06-08T10:30:00",
  "error": {
    "tool_name": "shell",
    "args": { "command": "cargo build" },
    "output": "error[E0308]: type mismatch...",
    "exit_code": 1,
    "duration_ms": 3450
  },
  "context": [
    {"role": "user", "content": "帮我修复编译错误"},
    {"role": "assistant", "content": "让我先看看代码...", "tool_calls": [...]},
    {"role": "tool", "tool_call_id": "call_1", "content": "// 代码内容..."}
  ],
  "task_context": {
    "plan": "## 步骤\n- [x] 1. 分析错误\n- [ ] 2. 修复代码\n- [ ] 3. 验证编译",
    "agenda": "当前步骤: 2. 修复代码",
    "turn": 5,
    "total_messages": 23
  }
}
```

### 3.4 快照大小控制

- context 只保留最近 6 条消息（≈ 最后 2-3 轮对话）
- 工具输出超过 2000 字符时截断
- 单个快照通常 < 10KB

---

