# 错误排查（Error Investigation）能力 — 技术方案 — investigate 工具

> 原文拆分自 `../replay-capability.md`。

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

