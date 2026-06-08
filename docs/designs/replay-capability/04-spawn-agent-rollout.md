# 错误排查（Error Investigation）能力 — 技术方案 — spawn_agent 集成与实施路线

> 原文拆分自 `../replay-capability.md`。

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
