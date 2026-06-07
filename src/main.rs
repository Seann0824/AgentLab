use std::env;
use anyhow;
use dotenvy;
use futures_util::{StreamExt, stream::FuturesUnordered};

use crate::{
    context::{ContextManager, ContextStrategy, TokenEstimator},
    model::{ChatMessage, ModelEvent, ToolCall},
    task::TaskManager,
    tools::{ToolManager, base_shell::BashShell, edit_tool::EditTool, read_tool::ReadTool, subagent::SpawnAgent},
};

mod context;
mod model;
mod task;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let query_client = initial_model()?;
    let tool_manager = initial_tool_manager();
    let current_dir = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .display()
        .to_string();

    // ⭐ 解析 CLI 参数：--task 用于子 agent 单次运行模式
    let args: Vec<String> = std::env::args().collect();
    let single_task = if args.len() > 1 && args[1] == "--task" {
        if args.len() > 2 {
            Some(args[2..].join(" "))
        } else {
            eprintln!("⚠️  --task 参数需要提供任务描述");
            None
        }
    } else {
        None
    };

    let policy_summary = String::new(); // 权限摘要（后续可从配置加载）

    // ⭐ 定义上下文窗口策略
    let strategy = ContextStrategy::Auto {
        token_limit: 128_000,
        max_turns: 20,
        trigger_ratio: 0.7,
        enable_async_summary: true,
        enable_tool_pruning: true,
        tool_pruning_keep_recent: 3,
        tool_pruning_max_output_chars: 200,
    };
    let token_limit = strategy.token_limit().unwrap_or(128_000);

    // ⭐ 使用 ContextManager 替代 Vec<ChatMessage>
    let system_prompt = format!(
        r#"你当前工作的目录为 {current_dir}。这个目录是你模型的Agent架子，它构建你和外部世界沟通的 bridge。如果你需要什么能力自己修改agent代码补充。

{policy_summary}

【上下文管理说明】
- 为了管理上下文窗口，早期对话历史可能会被自动压缩为摘要。
- 摘要会按「目标 → 操作 → 决策 → 状态」的结构保留关键信息。
- 如果发现某些上下文缺失，请基于摘要信息继续工作。
- 重要的上下文信息请**写入文件**，而不是仅依赖对话历史。
- 系统状态信息（如 Token 使用率）会输出到 stderr，不会混入你的工具执行结果。

【工作原则】
- 读取文件内容后，关键信息应记录在文件中，不要仅依赖对话记忆。
- 如果需要在多轮对话中保持状态，请使用文件持久化。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【结构化工作流程】

当你接收到需要多步执行的复杂任务时，请遵循以下流程：

1. 【🧠 规划阶段】先输出分析，然后创建 PLAN.md 文件：
   - 目标描述
   - 执行步骤（编号列表，每步一个 checkbox：- [ ]）
   - 每个步骤的验证标准
   - 将当前步骤写入 AGENDA.md

2. 【🔧 执行阶段】按 PLAN.md 的步骤逐个执行：
   - 每完成一步，更新 PLAN.md 标记为 - [x]
   - 更新 AGENDA.md 反映最新进度
   - 遇到错误时，先分析原因再修复
   - 重要发现记录到 MEMORY.md

3. 【✅ 验证阶段】每次代码修改后必须验证：
   - 修改 Rust 代码后 → 运行 `cargo check 2>&1 | tail -30`
   - 修改配置文件后 → 检查语法完整性
   - 验证失败时：分析错误 → 修复 → 再次验证
   - 如果连续 3 次修复失败，重新规划方案

4. 【📝 总结阶段】所有步骤完成后向用户总结：
   - 完成了什么
   - 关键决策
   - 当前项目状态

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【记忆与状态管理】

项目根目录下维护以下状态文件：

📄 PLAN.md   — 当前执行计划（步骤列表 + 完成状态）
📄 AGENDA.md  — 当前议程精简版（任务名 + 进度 + 当前步骤）
📄 MEMORY.md  — 重要发现、关键决策、已知问题

规则：
- 每次开启新任务时，检查并读取这些文件恢复上下文
- 上下文被压缩后，通过读取这些文件重新理解当前状态
- 不要在对话中重复记录已写入文件的信息

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【自我进化】

你可以修改自身代码来增强能力：
1. 添加新工具 → 实现 Tool trait，注册到 ToolManager
2. 修改现有工具 → 增强已有工具的能力
3. 修复 bug → 修改对应 src/ 下的代码

规则：
- 修改代码后必须运行 cargo check 验证
- 验证通过后，新能力立即生效（下次调用时）
- 如果修改导致编译失败，分析错误并修复
- 重大改动建议分步进行（先加框架，再填实现）

当前可用工具：
- shell: 运行本地 CLI 命令（zsh）
- edit: 增量编辑文件（search_replace / insert / delete / append）
- read: 读取文件内容（支持行号范围）
- spawn_agent: 编译当前 agent 并派生子进程执行任务，用于验证代码修改

  【使用场景】修改自身代码后，编译新版本并派生子 agent 验证改动的效果：
  1. 先修改代码
  2. 使用 spawn_agent(task="具体验证任务", timeout_seconds=300)
  3. 工具会自动 cargo build，然后以 `--task` 模式启动子 agent
  4. 子 agent 独立完成任务后输出结果
  5. 主 agent 分析结果判断修改是否按预期工作

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
【任务状态管理】

系统内置了 TaskManager，它会：
1. 自动维护 PLAN.md / docs/AGENDA.md / docs/MEMORY.md 中的任务状态
2. 当上下文被压缩后，自动将当前任务状态注入到你的上下文中
3. 你可以通过 edit 工具直接编辑这些文件来更新任务状态

关键文件：
- 📄 PLAN.md     — 当前执行计划（步骤列表 + 完成状态）
- 📄 AGENDA.md   — 当前议程（任务名 + 进度 + 当前步骤）
- 📄 MEMORY.md   — 重要发现、关键决策、已知问题

规则：
- 当你开始一个新任务时，先规划步骤写入 PLAN.md
- 每完成一步，更新 PLAN.md 和 AGENDA.md
- 重要发现写入 MEMORY.md
- 上下文被压缩后，检查注入的任务状态恢复上下文"#,
        current_dir = current_dir,
        policy_summary = policy_summary,
    );

    let mut ctx = ContextManager::new(system_prompt, strategy);

    // ⭐ 启动异步摘要后台任务（可选，需要 ModelAdapter 支持）
    // 如果希望启用 LLM 摘要，传入 Some(query_client.clone())
    // 如果只用规则摘要，传入 None
    ctx.setup_summary_channel(None);

    // ⭐ 初始化任务管理器（结构化任务执行框架）
    let mut task_manager = TaskManager::new(&current_dir);
    task_manager.load();

    let mut is_auto = false;
    let mut terminal_line_dirty = false;
    let mut single_task_used = false; // 标记 --task 模式的首次输入是否已使用

    loop {
        if !is_auto {
            // ⭐ --task 模式：使用 CLI 参数作为首次输入
            if let Some(ref task) = single_task {
                if !single_task_used {
                    single_task_used = true;
                    let input = task.clone();
                    eprintln!("[子 agent] 执行任务: {}", &input);
                    ctx.add_message(ChatMessage::user(&input));
                    task_manager.on_user_input(&input);
                } else {
                    // --task 模式：任务已完成，退出
                    break Ok(());
                }
            } else {
                // 正常交互模式：从 stdin 读取
                let mut user_input = String::new();
                finish_terminal_line(&mut terminal_line_dirty);
                print!(">");
                std::io::Write::flush(&mut std::io::stdout())?;
                if std::io::stdin().read_line(&mut user_input).is_err() {
                    continue;
                }
                if user_input.trim().is_empty() {
                    continue;
                }
                let input = user_input.trim().to_string();
                ctx.add_message(ChatMessage::user(&input));
                task_manager.on_user_input(&input);
            }
        }

        // ⭐ 检查是否有异步摘要结果需要注入（摘要注入说明发生了压缩）
        let injected = ctx.poll_summary_results();
        let compressed = injected > 0;
        if injected > 0 {
            // 使用 eprint! 输出到 stderr，避免被 Shell 工具捕获
            eprintln!("\r\x1b[2K📋 异步摘要已生成并注入上下文 ({} 条)", injected);
        }

        // ⭐ 如果发生了压缩，注入当前任务状态到上下文（让模型知道做到哪了）
        if compressed || ctx.stats().compressed {
            if let Some(task_msg) = task_manager.get_inject_message() {
                ctx.add_message(task_msg);
                eprintln!("\r\x1b[2K📋 已注入当前任务状态（帮助模型恢复上下文）");
            }
        }

        // ⭐ 显示当前的 Token 使用状态（输出到 stderr）
        let stats = ctx.stats().clone();
        if stats.usage_ratio > 0.3 {
            eprint!(
                "\r\x1b[2K[Token: {}/{} ({:.0}%) | 保留 {} 条重要消息] ",
                TokenEstimator::format_tokens(stats.estimated_tokens),
                TokenEstimator::format_tokens(token_limit),
                stats.usage_ratio * 100.0,
                stats.preserved_count,
            );
        }

        let mut stream_chat = query_client.stream_chat(
            &ctx.get_messages(),
            tool_manager.get_tools_scehma(),
        );
        let mut tool_tasks = FuturesUnordered::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut final_assistant_message = String::new();
        let mut has_tool_calls = false;

        while let Some(model_event) = stream_chat.next().await {
            match model_event {
                ModelEvent::Text(content) => {
                    print!("{}", content);
                    terminal_line_dirty = !content.ends_with('\n');
                }
                ModelEvent::Thinking(content) => {
                    print!("\x1b[90m{}\x1b[0m", content);
                    terminal_line_dirty = !content.ends_with('\n');
                }
                ModelEvent::ToolCallBlock {
                    id,
                    name,
                    arguments,
                } => {
                    finish_terminal_line(&mut terminal_line_dirty);

                    // ===== Tool call visualization =====
                    println!("\x1b[36m━━━ 🔧 调用工具: {}\x1b[0m", name);
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&arguments) {
                        if name == "shell" {
                            if let Some(cmd) = args["command"].as_str() {
                                println!("\x1b[33m  $ {}\x1b[0m", cmd);
                            }
                        } else {
                            println!(
                                "\x1b[33m  {}\x1b[0m",
                                serde_json::to_string_pretty(&args).unwrap_or_default()
                            );
                        }
                    }
                    print!("\x1b[33m⏳ 正在执行...\x1b[0m");
                    std::io::Write::flush(&mut std::io::stdout())?;
                    // ===================================

                    has_tool_calls = true;
                    let tool_call = ToolCall {
                        id,
                        name,
                        arguments,
                    };
                    tool_calls.push(tool_call.clone());
                    tool_tasks.push(tool_manager.run(tool_call));
                }
                ModelEvent::Done(assistant_message) => {
                    final_assistant_message = assistant_message;
                }
                _ => (),
            }
            std::io::Write::flush(&mut std::io::stdout())?;
        }
        finish_terminal_line(&mut terminal_line_dirty);

        let tool_call_ids = tool_calls
            .iter()
            .map(|tool_call| tool_call.id.clone())
            .collect::<Vec<_>>();

        if tool_calls.len() > 0 {
            ctx.add_message(ChatMessage::assistant_tool_calls(
                final_assistant_message,
                tool_calls,
            ));
            is_auto = true;
        } else {
            ctx.add_message(ChatMessage::assistant(final_assistant_message));
            is_auto = false;
        }

        let mut tool_results = Vec::new();
        while let Some(tool_result) = tool_tasks.next().await {
            tool_results.push(tool_result);
        }

        // Clear loading line and render tool results
        if has_tool_calls {
            print!("\r\x1b[K");
            for tool_result in &tool_results {
                render_tool_result_from_msg(tool_result);
            }
        }

        // 将工具结果加入消息
        for tool_call_id in tool_call_ids {
            if let Some(index) = tool_results
                .iter()
                .position(|message| message.tool_call_id() == Some(tool_call_id.as_str()))
            {
                let tool_result = tool_results.remove(index);

                // ⭐ 如果是关键的工具结果（文件读取等），标记为重要
                let is_important = is_important_tool_result(&tool_result);

                ctx.add_message(tool_result);

                // 对重要工具结果，标记前一条消息（即刚添加的 tool 消息）为 preserved
                if is_important {
                    ctx.preserve_last_message();
                }
            }
        }
        // 剩余的 tool_results（没有对应 tool_call_id 的）
        for tool_result in tool_results {
            ctx.add_message(tool_result);
        }
    }
}

fn render_tool_result(content: &str) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        let ok = value["ok"].as_bool().unwrap_or(false);
        if ok {
            if let Some(result) = value.get("result") {
                if result.is_object() {
                    let success = result["success"].as_bool().unwrap_or(true);
                    let status = result["status"].as_i64();
                    if success {
                        println!(
                            "\x1b[32m━━━ ✅ 执行成功 (exit: {}) ━━━\x1b[0m",
                            status.unwrap_or(0)
                        );
                    } else {
                        println!(
                            "\x1b[31m━━━ ❌ 执行失败 (exit: {}) ━━━\x1b[0m",
                            status.unwrap_or(-1)
                        );
                    }
                    if let Some(stdout) = result["stdout"].as_str() {
                        if !stdout.is_empty() {
                            print!("{}", stdout);
                            if !stdout.ends_with('\n') {
                                println!();
                            }
                        }
                    }
                    if let Some(stderr) = result["stderr"].as_str() {
                        if !stderr.is_empty() {
                            print!("\x1b[31m{}\x1b[0m", stderr);
                            if !stderr.ends_with('\n') {
                                println!();
                            }
                        }
                    }
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(result).unwrap_or_default()
                    );
                }
            }
        } else {
            println!("\x1b[31m━━━ ❌ 工具调用失败 ━━━\x1b[0m");
            if let Some(error) = value.get("error") {
                println!(
                    "\x1b[31m  {}\x1b[0m",
                    error["message"].as_str().unwrap_or("unknown error")
                );
            }
        }
    }
}


/// 从 ChatMessage 中提取 content 并渲染工具结果
fn render_tool_result_from_msg(msg: &ChatMessage) {
    if let ChatMessage::Tool { content, .. } = msg {
        render_tool_result(content);
    }
}

/// 判断工具结果是否为重要上下文（文件列表、项目结构等）
fn is_important_tool_result(msg: &ChatMessage) -> bool {
    let ChatMessage::Tool { content, .. } = msg else { return false };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(content) else { return false };
    let Some(stdout) = val
        .get("result")
        .and_then(|r| r.get("stdout"))
        .and_then(|s| s.as_str())
    else { return false };

    context::is_stdout_structural(stdout)
}

fn finish_terminal_line(terminal_line_dirty: &mut bool) {
    if *terminal_line_dirty {
        println!();
        *terminal_line_dirty = false;
    }
}

fn initial_model() -> anyhow::Result<Box<dyn model::ModelAdapter>> {
    // 1. 读取环境变量
    dotenvy::dotenv().ok();

    let api_key = env::var("DEEPSEEK_API_KEY")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY not set"))?;
    let deepseek_base_url = env::var("DEEPSEEK_BASE_URL")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_BASE_URL not set"))?;

    let openai_adapter = model::OpenAiCompatibleAdapter::new(
        deepseek_base_url,
        api_key,
        "deepseek-v4-flash".to_string(),
    );

    Ok(Box::new(openai_adapter))
}

fn initial_tool_manager() -> ToolManager {
    let mut tool_manager = ToolManager::new();
    tool_manager.register_tool(Box::new(BashShell));
    tool_manager.register_tool(Box::new(EditTool));
    tool_manager.register_tool(Box::new(ReadTool));
    tool_manager.register_tool(Box::new(SpawnAgent));
    tool_manager
}
