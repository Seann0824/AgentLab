// src/tools/subagent/mod.rs
//
// spawn_agent 工具 — 编译当前 agent 并派生子进程执行指定任务
//
// 核心用途：
// 1. Agent 修改自身代码后，编译新版本
// 2. 派生子 agent 进程执行验证任务
// 3. 收集子 agent 的执行结果，判断修改是否按预期工作
//
// 使用场景（自我迭代验证）：
// - 修改了某个工具，需要验证它仍能正确工作
// - 修改了上下文压缩逻辑，需要验证 token 管理正常
// - 新增了功能，需要验证端到端流程

use std::process::Stdio;
use std::time::Duration;

use tokio::{io::AsyncReadExt, process::Command, sync::mpsc, time};
use tokio_stream::wrappers::ReceiverStream;

use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// spawn_agent 工具
///
/// 编译当前 agent 代码，派生子进程执行指定任务，并返回执行结果。
pub struct SpawnAgent;

impl Tool for SpawnAgent {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "编译当前 agent，派生子 agent 进程执行指定任务，用于验证代码修改是否按预期工作。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "spawn_agent",
                "description": "编译当前 agent 代码，派生子进程执行指定任务，返回子 agent 的完整输出。用于自我迭代验证。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "子 agent 需要执行的任务描述。会作为用户输入传递给子 agent。"
                        },
                        "timeout_seconds": {
                            "type": "integer",
                            "description": "超时秒数（默认 300，即 5 分钟）。超过此时间子进程将被终止。",
                            "default": 300
                        }
                    },
                    "required": ["task"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let task = args["task"].as_str().unwrap_or("").to_string();
        let timeout_secs = args["timeout_seconds"].as_u64().unwrap_or(300);

        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            if task.trim().is_empty() {
                let _ = tx
                    .send(ToolEvent::Err("task description is empty".to_string()))
                    .await;
                return;
            }

            // === Step 1: 编译 agent ===
            let _ = tx
                .send(ToolEvent::Progress("📦 正在编译 agent...".to_string()))
                .await;

            let build_result = time::timeout(
                Duration::from_secs(120), // 编译超时 2 分钟
                Command::new("cargo")
                    .args(["build"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output(),
            )
            .await;

            let build_output = match build_result {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => {
                    let _ = tx
                        .send(ToolEvent::Err(format!("编译失败（启动错误）: {}", e)))
                        .await;
                    return;
                }
                Err(_) => {
                    let _ = tx
                        .send(ToolEvent::Err("编译超时（超过 120 秒）".to_string()))
                        .await;
                    return;
                }
            };

            if !build_output.status.success() {
                let stderr = String::from_utf8_lossy(&build_output.stderr);
                let _ = tx
                    .send(ToolEvent::Err(format!(
                        "编译失败:\n{}",
                        stderr.chars().take(2000).collect::<String>()
                    )))
                    .await;
                return;
            }

            let _ = tx
                .send(ToolEvent::Progress(
                    "✅ 编译成功，正在派生子 agent...".to_string(),
                ))
                .await;

            // === Step 2: 启动子 agent 进程 ===
            let binary_path = if cfg!(debug_assertions) {
                "./target/debug/agent-lab"
            } else {
                "./target/release/agent-lab"
            };

            let mut child = match Command::new(binary_path)
                .arg("--task")
                .arg(&task)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    let _ = tx
                        .send(ToolEvent::Err(format!("无法启动子 agent: {}", e)))
                        .await;
                    return;
                }
            };

            // === Step 3: 等待子进程完成（带超时） ===
            // 保存子进程 PID 和输出管道，用于超时后强制终止和手动读取
            let child_pid = match child.id() {
                Some(pid) => pid,
                None => {
                    let _ = tx
                        .send(ToolEvent::Err("无法获取子进程 PID".to_string()))
                        .await;
                    return;
                }
            };

            // 分离 stdout/stderr 读取端（在 wait 之前取出，避免所有权问题）
            let child_stdout = child.stdout.take();
            let child_stderr = child.stderr.take();

            // 等待子进程退出（不消耗 stdout/stderr）
            let timeout_duration = Duration::from_secs(timeout_secs);
            let wait_result = time::timeout(timeout_duration, child.wait()).await;

            let (stdout, stderr, exit_code, success) = match wait_result {
                Ok(Ok(status)) => {
                    // 子进程已退出，手动读取输出
                    let stdout = if let Some(mut out) = child_stdout {
                        let mut buf = String::new();
                        let _ = out.read_to_string(&mut buf).await;
                        buf
                    } else {
                        String::new()
                    };

                    let stderr = if let Some(mut err) = child_stderr {
                        let mut buf = String::new();
                        let _ = err.read_to_string(&mut buf).await;
                        buf
                    } else {
                        String::new()
                    };

                    let exit_code = status.code().unwrap_or(-1);
                    let success = status.success();
                    (stdout, stderr, exit_code, success)
                }
                Ok(Err(e)) => {
                    let _ = tx
                        .send(ToolEvent::Err(format!("子 agent 执行出错: {}", e)))
                        .await;
                    return;
                }
                Err(_) => {
                    // 超时：强制终止子进程
                    let _ = std::process::Command::new("kill")
                        .args(["-9", &child_pid.to_string()])
                        .output();
                    // 再尝试读取部分输出
                    let stdout = if let Some(mut out) = child_stdout {
                        let mut buf = String::new();
                        let _ = out.read_to_string(&mut buf).await;
                        buf
                    } else {
                        String::new()
                    };
                    let _ = tx
                        .send(ToolEvent::Err(format!(
                            "子 agent 执行超时（超过 {} 秒），已终止 (PID: {})\n部分输出:\n{}",
                            timeout_secs, child_pid, stdout
                        )))
                        .await;
                    return;
                }
            };

            let summary = format!(
                "子 agent 执行完成 (exit: {})，共输出 {} 字符",
                exit_code,
                stdout.len() + stderr.len(),
            );
            let _ = tx
                .send(ToolEvent::Done(serde_json::json!({
                    "exit_code": exit_code,
                    "success": success,
                    "stdout": stdout,
                    "stderr": stderr,
                    "summary": summary,
                })))
                .await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
