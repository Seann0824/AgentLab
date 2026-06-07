# 错误排查（Error Investigation）能力 — 技术方案

> **创建日期**: 2025-06-08
> **状态**: ⏳ 方案设计
> **版本**: v2（简化版，聚焦核心场景）
>
> **核心思想**: 不搞复杂的回放引擎，只做一件事——工具调用报错时，自动保存当时的上下文快照，
> 提供一个 `investigate` 工具让 LLM 能回到「错误现场」排查分析。

---

## 目录

1. [核心场景](#1-核心场景)
2. [整体设计](#2-整体设计)
3. [组件一：错误快照（ErrorSnapshot）](#3-组件一错误快照errorsnapshot)
4. [组件二：investigate 工具](#4-组件二investigate-工具)
5. [组件三：spawn_agent 自动集成](#5-组件三spawn_agent-自动集成)
6. [实施路线](#6-实施路线)
7. [验证标准](#7-验证标准)
8. [文件变更清单](#8-文件变更清单)

---

## 1. 核心场景

### 场景一：工具调用报错（主场景）

```
Agent 调用 shell("cargo build") → 编译失败 (exit code 1)
                                  ↓
自动保存快照: 当前上下文消息 + 错误信息 + 当前任务状态
                                  ↓
Agent 调用 investigate("snapshot_001")
                                  ↓
LLM 在快照上下文中分析 → 输出「哪里错、为什么错、怎么修」
```

### 场景二：spawn_agent 子进程失败

```
spawn_agent(task="fix-bug")
  → 子 agent 执行
  → 子 agent 报错退出（exit code != 0）
  → 自动保存子 agent 的上下文快照
  → 自动触发 investigate 分析
  → 返回结果自带根因分析
```

### 场景三：事后排查

```
用户: "刚才那个编译错误是怎么回事？"
Agent: 调 investigate("snapshot_002") 查看快照
       → 看到当时完整上下文 + 错误信息
       → 输出排查报告
```

---

## 2. 整体设计

### 2.1 设计原则

1. **只记录错误现场** — 不记录每一步，只在出错时自动保存快照
2. **轻量** — 一个错误快照就是一份 JSON 文件，存关键上下文片段（不是全量消息）
3. **排查即分析** — `investigate` 工具就是把快照喂给 LLM，让 LLM 在完整上下文中做根因分析
4. **自动触发** — spawn_agent 失败自动走排查流程

### 2.2 数据流

```
agent.rs 主循环
  │
  │  工具执行 ↓
  ▼
工具返回错误 (exit code != 0 / ToolEvent::Err)
  │
  │  自动 ↓
  ▼
ErrorSnapshot::capture()
  ├── 截取当前 ContextManager 的关键消息（最后 N 轮）
  ├── 记录错误信息（工具名、参数、错误输出）
  ├── 记录当时任务状态（PLAN.md / AGENDA.md 内容）
  └── 保存到 .agent/snapshots/snapshot_001.json
  │
  │  Agent 排查 ↓
  ▼
investigate("snapshot_001")
  ├── 加载快照
  ├── 构造排查 Prompt（包含：错误信息 + 上下文消息 + 任务状态）
  ├── 调用 LLM 做根因分析
  └── 输出排查报告
```

### 2.3 触发点

在 `agent.rs` 主循环中，工具执行完毕后检查结果：

```rust
// 在 agent.rs run() 中，收集完 tool_results 后
for tool_result in &tool_results {
    if is_error_result(tool_result) {
        // ⭐ 自动保存错误快照
        let snapshot = ErrorSnapshot::capture(
            &self.context_manager,
            &self.task_manager,
            tool_result,
        )?;
        snapshot.save()?;
        // 后续 Agent 可以通过 investigate 工具查看
    }
}
```

---

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

## 4. 组件二：investigate 工具

### 4.1 工具接口

```rust
pub struct InvestigateTool;

// 工具名称和描述
name: "investigate"
description: "加载一个错误快照，在错误现场上下文中分析错误根因。当工具调用报错后使用。"
```

**参数 schema：**
```json
{
  "type": "object",
  "properties": {
    "snapshot_id": {
      "type": "string",
      "description": "要分析的错误快照 ID（如 '20250608_103000'）。查看所有快照用 'list'"
    },
    "focus": {
      "type": "string",
      "enum": ["root_cause", "fix", "all"],
      "description": "分析重点：root_cause（只分析根因）、fix（分析如何修复）、all（完整分析）",
      "default": "all"
    }
  },
  "required": ["snapshot_id"]
}
```

### 4.2 执行逻辑

```rust
fn execute(&self, args: serde_json::Value) -> ToolStream {
    let snapshot_id = args["snapshot_id"].as_str().unwrap();
    let focus = args["focus"].as_str().unwrap_or("all");
    let model = self.model.clone(); // 持有 ModelAdapter 引用

    tokio::spawn(async move {
        // 如果是 list，列出所有快照
        if snapshot_id == "list" {
            let list = manager.list()?;
            tx.send(ToolEvent::Done(json!(list))).await;
            return;
        }

        // 1. 加载快照
        let snapshot = manager.load(snapshot_id)?;

        // 2. 构造排查 prompt
        let prompt = build_investigation_prompt(&snapshot, focus);

        // 3. 调用 LLM 分析
        let result = model.simple_chat(&[
            ChatMessage::system("你是一个调试专家。你会基于完整的错误现场上下文分析问题。"),
            ChatMessage::user(&prompt),
        ]).await?;

        // 4. 返回分析结果
        tx.send(ToolEvent::Done(json!({
            "snapshot_id": snapshot_id,
            "tool": snapshot.error.tool_name,
            "analysis": result,
        }))).await;
    });
}
```

### 4.3 排查 Prompt 模板

```rust
fn build_investigation_prompt(snapshot: &ErrorSnapshot, focus: &str) -> String {
    format!(
        r#"你是一个调试专家。以下是工具调用报错时的完整现场信息。

━━━ 错误信息 ━━━
工具: {tool_name}
参数: {args}
退出码: {exit_code}
执行耗时: {duration_ms}ms

━━━ 错误输出 ━━━
```
{output}
```

━━━ 报错前的上下文消息 ━━━
以下是报错前关键轮次的对话消息：

{context_messages}

━━━ 任务状态（报错时）━━━
PLAN.md:
{plan_content}

AGENDA.md:
{agenda_content}

━━━ 分析要求 ━━━
请基于以上完整上下文，输出排查报告：

1. **错误定位**：具体是什么报错？在什么操作下发生的？
2. **根因分析**：为什么会发生这个错误？是代码问题、环境问题还是逻辑问题？
3. **上下文链**：之前的哪些操作/决策导致了当前错误？
4. **修复方案**：具体的修复步骤是什么？
5. **预防建议**：如何避免未来再次出现类似错误？"#,
        tool_name = snapshot.error.tool_name,
        args = serde_json::to_string_pretty(&snapshot.error.args).unwrap_or_default(),
        exit_code = snapshot.error.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "N/A".into()),
        duration_ms = snapshot.error.duration_ms,
        output = snapshot.error.output,
        context_messages = format_context_messages(&snapshot.context),
        plan_content = snapshot.task_context.plan.as_deref().unwrap_or("(无)"),
        agenda_content = snapshot.task_context.agenda.as_deref().unwrap_or("(无)"),
    )
}
```

### 4.4 输出示例

```
━━━ 🔬 错误排查报告 ━━━

快照: 20250608_103000
工具: shell (cargo build)
退出码: 1

1️⃣ 错误定位
编译错误 E0308：类型不匹配，在 src/db/postgres.rs:42 行
期望类型 `Result<Vec<User>>`，实际返回 `Vec<User>`

2️⃣ 根因分析
第 3 轮修改了 `DatabaseTrait::query()` 的返回签名
  从 `fn query() -> Vec<User>` 改为 `fn query() -> Result<Vec<User>>`
第 4 轮更新了 PostgresDatabase 实现的方法签名
  但第 42 行的 `return users;` 忘记改为 `return Ok(users);`

3️⃣ 上下文链
  第3轮: edit 修改 trait 定义 ✓
  第4轮: edit 修改 impl 方法签名 ✓
  第5轮: ❌ shell(cargo build) → 编译错误
  问题: 修改方法签名后，方法体内部的 return 语句没有同步更新

4️⃣ 修复方案
  编辑 src/db/postgres.rs:42
  将 `return users;` 改为 `return Ok(users);`
  然后运行 cargo check 验证

5️⃣ 预防建议
  修改方法签名后，应使用 search 搜索所有 return 语句
  确保返回值类型与新签名一致
```

---

## 5. 组件三：spawn_agent 自动集成

### 5.1 子 agent 侧：报错时输出快照 ID

```rust
// agent.rs 的 --task 模式结束时
// 如果有错误发生，将快照 ID 输出到 stderr
let snapshot_manager = ErrorSnapshotManager::new(&self.current_dir);
for turn_result in &turn_results {
    if turn_result.has_error {
        let snapshot = snapshot_manager.capture(
            &self.context_manager,
            &self.task_manager,
            turn_result.error_info(),
        )?;
        let path = snapshot_manager.save(&snapshot)?;
        // 父进程通过 stderr 捕获此信息
        eprintln!("[SNAPSHOT] {}", snapshot.id);
        break; // 只保存第一个错误的快照
    }
}
```

### 5.2 spawn_agent 侧：捕获并自动排查

```rust
// src/tools/subagent/mod.rs
pub struct SpawnAgent {
    model: Option<Box<dyn ModelAdapter>>,  // 持有模型引用
}

fn execute(&self, args: serde_json::Value) -> ToolStream {
    tokio::spawn(async move {
        // ... 编译 + 执行子 agent ...

        // 捕获 stderr 中的快照 ID
        let snapshot_id = extract_snapshot_id(&stderr);

        if !success && snapshot_id.is_some() && self.model.is_some() {
            // 自动触发排查分析
            let analysis = auto_investigate(
                snapshot_id.unwrap(),
                &self.model,
                &self.current_dir,
            ).await;

            // 返回结果 + 自动排查报告
            tx.send(ToolEvent::Done(json!({
                "exit_code": exit_code,
                "success": false,
                "stdout": stdout,
                "stderr": stderr,
                "investigation": analysis,
            }))).await;
        } else {
            tx.send(ToolEvent::Done(json!({
                "exit_code": exit_code,
                "success": success,
                "stdout": stdout,
                "stderr": stderr,
            }))).await;
        }
    });
}

/// 从 stderr 中提取快照 ID
fn extract_snapshot_id(stderr: &str) -> Option<String> {
    stderr.lines()
        .find(|line| line.starts_with("[SNAPSHOT] "))
        .map(|line| line.trim_start_matches("[SNAPSHOT] ").to_string())
}
```

### 5.3 自动排查实现

```rust
/// 自动加载快照并调用 LLM 排查
async fn auto_investigate(
    snapshot_id: String,
    model: &Arc<Mutex<Box<dyn ModelAdapter>>>,
    current_dir: &str,
) -> anyhow::Result<serde_json::Value> {
    let manager = ErrorSnapshotManager::new(Path::new(current_dir));
    let snapshot = manager.load(&snapshot_id)?;

    let prompt = format!(
        r#"子 agent 执行失败。以下是从错误现场捕获的快照。

错误工具: {tool}
错误输出: {output}
退出码: {exit_code}

以下是报错前的上下文消息：
{context}

请快速诊断：
1. 发生了什么错误？
2. 最可能的根因是什么？
3. 修复建议？"#,
        tool = snapshot.error.tool_name,
        output = snapshot.error.output.chars().take(1000).collect::<String>(),
        exit_code = snapshot.error.exit_code.map(|c| c.to_string()).unwrap_or_default(),
        context = format_context_messages(&snapshot.context),
    );

    let model = model.lock().await;
    let result = model.simple_chat(&[
        ChatMessage::system("你是一个调试专家，快速诊断子 agent 失败原因。"),
        ChatMessage::user(&prompt),
    ]).await?;

    Ok(json!({
        "snapshot_id": snapshot_id,
        "tool": snapshot.error.tool_name,
        "diagnosis": result,
    }))
}
```

---

## 6. 实施路线

### 阶段一：错误快照 + investigate 工具（P0 · ~1天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 1.1 | 实现 `ErrorSnapshot` 数据类型 | `src/investigate/types.rs` |
| 1.2 | 实现 `ErrorSnapshotManager`（捕获、保存、加载、列表） | `src/investigate/mod.rs` |
| 1.3 | 在 `agent.rs` 主循环中嵌入自动捕获点（工具报错时触发） | agent 自动保存快照 |
| 1.4 | 实现 `InvestigateTool`（加载快照 + 构造 prompt + 调用 LLM） | `src/tools/investigate/mod.rs` |
| 1.5 | 注册到 ToolManager | 工具可用 |
| 1.6 | 验证：工具报错后调 investigate 能输出分析 | 功能验证 |

### 阶段二：spawn_agent 自动集成（P1 · ~0.5天）

| 步骤 | 内容 | 产出 |
|------|------|------|
| 2.1 | 子 agent `--task` 模式报错时输出 `[SNAPSHOT]` 标记 | 父进程可捕获 |
| 2.2 | spawn_agent 捕获快照 ID + 持有 ModelAdapter | 可调用 LLM |
| 2.3 | 实现自动排查分析 | 失败返回自带诊断 |
| 2.4 | 端到端验证 | 闭环形成 |

---

## 7. 验证标准

| 场景 | 预期 | 验证方式 |
|------|------|----------|
| shell 命令返回非 0 | `snapshots/index.json` 新增一条记录 | `ls .agent/snapshots/` |
| 调 investigate("list") | 列出所有错误快照摘要 | 控制台输出 |
| 调 investigate("xxx") | LLM 输出包括：错误定位、根因、修复方案 | 控制台输出分析 |
| spawn_agent 子进程报错 | 返回结果包含 `investigation` 字段 | 查看返回 JSON |
| 快照中的上下文正确 | context 包含报错前的关键轮次消息 | 验证 JSON 内容 |

---

## 8. 文件变更清单

### 新增文件

| 文件 | 说明 |
|------|------|
| `src/investigate/mod.rs` | ErrorSnapshotManager 实现（捕获、保存、加载、列表） |
| `src/investigate/types.rs` | ErrorSnapshot、ErrorInfo、TaskContextSnapshot 数据类型 |
| `src/tools/investigate/mod.rs` | InvestigateTool 工具实现 |
| `docs/designs/replay-capability.md` | 本文档 |

### 修改文件

| 文件 | 修改内容 |
|------|----------|
| `src/lib.rs` | 添加 `pub mod investigate;` |
| `src/agent.rs` | 主循环中工具报错后自动捕获快照 |
| `src/tools/mod.rs` | 注册 InvestigateTool |
| `src/tools/subagent/mod.rs` | 持有 ModelAdapter，捕获 `[SNAPSHOT]` 标记，自动触发排查 |
| `Cargo.toml` | 可能需要添加 `chrono` |

---

> **总结**：本方案只聚焦一个核心场景——工具调用报错时自动保存「错误现场」快照，然后提供一个 `investigate` 工具让 LLM 在完整上下文中做根因分析。不搞复杂的回放引擎，不做三种模式，让排查能力以最轻量的方式落地。
