# 工具调用感知能力 技术方案（简化版）

> 解决"使用的时候无法感知到工具调用"的问题

## 1. 问题分析

### 1.1 当前行为

```
> 帮我看看当前目录有什么文件
[模型开始输出文字...]
（几秒后突然出现工具执行结果，用户不知道发生了什么）
```

### 1.2 根因

在 `main.rs` 的事件循环中，`ModelEvent::ToolCallBlock` 的处理是"静默"的：

```rust
ModelEvent::ToolCallBlock { id, name, arguments } => {
    let tool_call = ToolCall { id, name, arguments };
    tool_calls.push(tool_call.clone());
    tool_tasks.push(tool_manager.run(tool_call));
    // ❌ 没有任何终端输出！
},
```

用户完全看不到：
- 模型调用了什么工具、传递了什么参数
- 工具正在执行中（进度未知）
- 工具是成功还是失败

## 2. 方案设计（简化版）

去掉流式进度输出，工具调用时**展示 loading 动画**，执行完毕后**呈现结果**。

### 2.1 分层策略

```
┌──────────────────────────────────────────────┐
│  层1: Tool Call 通知（即时感知）               │
│  - 工具名称 + 参数预览                        │
│  - 使用特殊颜色/格式区分                      │
├──────────────────────────────────────────────┤
│  层2: 执行中 Loading（反馈工具在执行）          │
│  - 简单 spinner 动画                         │
│  - 或静态 "⏳ 正在执行..." 提示               │
├──────────────────────────────────────────────┤
│  层3: 工具结果呈现（清晰可读）                 │
│  - 成功/失败状态                             │
│  - 格式化输出结果                            │
└──────────────────────────────────────────────┘
```

### 2.2 核心改动点

#### 改动 1: main.rs — Tool Call 可视化 + Loading

在 `ModelEvent::ToolCallBlock` 分支中：

```rust
ModelEvent::ToolCallBlock { id, name, arguments } => {
    // 打印工具调用信息（带颜色）
    println!("\x1b[36m━━━ 🔧 调用工具: {}\x1b[0m", name);
    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&arguments) {
        if name == "shell" {
            if let Some(cmd) = args["command"].as_str() {
                println!("\x1b[33m  $ {}\x1b[0m", cmd);
            }
        }
    }
    // 启动 loading（使用非阻塞方式）
    print!("\x1b[33m⏳ 正在执行...\x1b[0m");
    std::io::Write::flush(&mut std::io::stdout())?;
    
    let tool_call = ToolCall { id, name, arguments };
    tool_calls.push(tool_call.clone());
    tool_tasks.push(tool_manager.run(tool_call));
}
```

#### 改动 2: main.rs — 等待工具执行完毕，清除 loading + 显示结果

当前流程中，工具结果是在 `while let Some(tool_result) = tool_tasks.next().await` 中收集的。
在这个阶段渲染结果，并清除 loading 提示：

```rust
// 清除 loading 行（使用 \r 回到行首 + 清除整行）
print!("\r\x1b[K");

// 然后显示结果
for tool_result in tool_results {
    // 根据 tool_result 内容打印成功/失败
    // 显示 stdout/stderr
}
```

#### 改动 3: main.rs — 结果呈现美化

```rust
// 遍历 tool_results 时，解析内容并美化输出
for tool_result in &tool_results {
    if let ChatMessage::Tool { content, .. } = tool_result {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
            let ok = value["ok"].as_bool().unwrap_or(false);
            if ok {
                if let Some(result) = value.get("result") {
                    let success = result["success"].as_bool().unwrap_or(true);
                    let status = result["status"].as_i64();
                    let stdout = result["stdout"].as_str().unwrap_or("");
                    let stderr = result["stderr"].as_str().unwrap_or("");
                    
                    if success {
                        println!("\x1b[32m━━━ ✅ 执行成功 (exit: {}) ━━━\x1b[0m", status.unwrap_or(0));
                    } else {
                        println!("\x1b[31m━━━ ❌ 执行失败 (exit: {}) ━━━\x1b[0m", status.unwrap_or(-1));
                    }
                    if !stdout.is_empty() { print!("{}", stdout); }
                    if !stderr.is_empty() { print!("\x1b[31m{}\x1b[0m", stderr); }
                }
            } else {
                println!("\x1b[31m━━━ ❌ 工具调用失败 ━━━\x1b[0m");
                if let Some(error) = value.get("error") {
                    println!("\x1b[31m{}\x1b[0m", error["message"].as_str().unwrap_or("unknown error"));
                }
            }
        }
    }
}
```

### 2.3 终端输出效果示例

```
> 帮我看看当前目录有什么文件
让我查看一下当前目录的内容。

━━━ 🔧 调用工具: shell ━━━━━━━━━━━━━━━━━━━━━━━━
  $ ls -la
⏳ 正在执行...                  ← 显示 loading，直到工具执行完毕
━━━ ✅ 执行成功 (exit: 0) ━━━━━━━━━━━━━━━━━━━━━
total 144
drwxr-xr-x@ 10 sean  staff    320 Jun  7 17:19 .
drwxr-xr-x@ 16 sean  staff    512 Jun  5 00:21 ..
...

当前目录下有以下文件和目录：
- Cargo.toml
- src/
- docs/
- ...
```

## 3. 具体代码变更

### 3.1 main.rs 变更

```rust
// main.rs 事件循环

while let Some(model_event) = stream_chat.next().await {
    match model_event {
        ModelEvent::Text(content) => {
            print!("{}", content);
            terminal_line_dirty = !content.ends_with('\n');
        },
        ModelEvent::Thinking(content) => {
            print!("\x1b[90m{}\x1b[0m", content);
            terminal_line_dirty = !content.ends_with('\n');
        },
        ModelEvent::ToolCallBlock { id, name, arguments } => {
            finish_terminal_line(&mut terminal_line_dirty);
            
            // 工具调用通知
            println!("\x1b[36m━━━ 🔧 调用工具: {}\x1b[0m", name);
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(&arguments) {
                if name == "shell" {
                    if let Some(cmd) = args["command"].as_str() {
                        println!("\x1b[33m  $ {}\x1b[0m", cmd);
                    }
                } else {
                    println!("\x1b[33m  {}\x1b[0m", serde_json::to_string_pretty(&args).unwrap_or_default());
                }
            }
            print!("\x1b[33m⏳ 正在执行...\x1b[0m");
            std::io::Write::flush(&mut std::io::stdout())?;
            
            let tool_call = ToolCall { id, name, arguments };
            tool_calls.push(tool_call.clone());
            tool_tasks.push(tool_manager.run(tool_call));
        },
        ModelEvent::Done(assistant_message) => {
            final_assistant_message = assistant_message;
        }
        _ => ()
    }
    std::io::Write::flush(&mut std::io::stdout())?;
}

// stream 结束后，等待工具任务
finish_terminal_line(&mut terminal_line_dirty);

// 收集工具结果
let mut tool_results = Vec::new();
while let Some(tool_result) = tool_tasks.next().await {
    tool_results.push(tool_result);
}

// 清除 loading 提示
print!("\r\x1b[K");

// 渲染工具结果
for tool_result in &tool_results {
    if let ChatMessage::Tool { content, .. } = tool_result {
        render_tool_result(content);
    }
}

// 将结果加入消息历史
for tool_call_id in tool_call_ids {
    if let Some(index) = tool_results
        .iter()
        .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
    {
        let tool_result = tool_results.remove(index);
        messages.push(tool_result);
    }
}
```

新增辅助函数：

```rust
fn render_tool_result(content: &str) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        let ok = value["ok"].as_bool().unwrap_or(false);
        if ok {
            if let Some(result) = value.get("result") {
                if result.is_object() {
                    let success = result["success"].as_bool().unwrap_or(true);
                    let status = result["status"].as_i64();
                    if success {
                        println!("\x1b[32m━━━ ✅ 执行成功 (exit: {}) ━━━\x1b[0m", status.unwrap_or(0));
                    } else {
                        println!("\x1b[31m━━━ ❌ 执行失败 (exit: {}) ━━━\x1b[0m", status.unwrap_or(-1));
                    }
                    if let Some(stdout) = result["stdout"].as_str() {
                        if !stdout.is_empty() {
                            print!("{}", stdout);
                            if !stdout.ends_with('\n') { println!(); }
                        }
                    }
                    if let Some(stderr) = result["stderr"].as_str() {
                        if !stderr.is_empty() {
                            print!("\x1b[31m{}\x1b[0m", stderr);
                            if !stderr.ends_with('\n') { println!(); }
                        }
                    }
                } else {
                    println!("{}", serde_json::to_string_pretty(result).unwrap_or_default());
                }
            }
        } else {
            println!("\x1b[31m━━━ ❌ 工具调用失败 ━━━\x1b[0m");
            if let Some(error) = value.get("error") {
                println!("\x1b[31m  {}\x1b[0m", error["message"].as_str().unwrap_or("unknown error"));
            }
        }
    }
}
```

### 3.2 BashShell 无需改动

因为不追求流式进度，`BashShell` 的行为保持不变——执行完毕后一次性返回 `ToolEvent::Done`。

### 3.3 ToolManager 无需改动

保持现有 `run` 方法签名不变，返回 `ChatMessage::Tool`。

## 4. main.rs 执行流程（更新后）

```
Stream Chat
  ↓
ModelEvent::ToolCallBlock
  ├── 打印工具调用通知（名称 + 命令）
  └── 显示 "⏳ 正在执行..."
  └── 启动工具任务（FuturesUnordered）
  ↓
ModelEvent::Text / Thinking 继续渲染
  ↓
ModelEvent::Done → 保存 assistant_message
  ↓
Stream 结束
  ↓
等待所有工具任务完成（FuturesUnordered 遍历）
  ↓
清除 loading 提示（\r\x1b[K）
  ↓
渲染工具结果（成功/失败 + stdout/stderr）
  ↓
结果加入 messages → 进入下一轮对话
```

## 5. 实现步骤

| 步骤 | 文件 | 改动内容 | 工作量 |
|------|-----|---------|--------|
| 1 | `main.rs` | ToolCallBlock 增加工具调用通知+loading | 小 |
| 2 | `main.rs` | 工具结果渲染函数 `render_tool_result` | 中 |
| 3 | `main.rs` | 结果收集后清除 loading 并展示结果 | 小 |

## 6. 风险与注意事项

1. **ANSI 兼容**：颜色码在 Windows 旧终端可能不支持，后续可加 `--no-color`
2. **多工具并发**：多个工具同时调用时，loading 只显示一个，可以通过加 `[工具名]` 前缀区分
3. **loading 行清除**：使用 `\r\x1b[K`（回车 + 清除行）确保 loading 被正确覆盖

## 7. 后续可扩展

- 动态 spinner 动画（`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` 轮转）
- 工具执行时间统计
- 多工具并发时的独立进度条
