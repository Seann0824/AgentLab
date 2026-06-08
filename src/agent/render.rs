use super::events::RunEventKind;
use super::output::{OutputMode, emit_json_event};
use crate::model::ChatMessage;

fn render_tool_result(tool_call_id: Option<&str>, content: &str, output_mode: OutputMode) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
        if output_mode.is_json() {
            render_tool_result_json(tool_call_id, value);
            return;
        }

        let ok = value["ok"].as_bool().unwrap_or(false);
        if ok {
            if let Some(result) = value.get("result") {
                if output_mode.is_full() {
                    if is_process_result(result) {
                        render_full_tool_result(result);
                    } else {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(result).unwrap_or_default()
                        );
                    }
                } else {
                    render_concise_tool_result(&value, result);
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
pub(super) fn render_tool_result_from_msg(msg: &ChatMessage, output_mode: OutputMode) {
    if let ChatMessage::Tool {
        tool_call_id,
        content,
    } = msg
    {
        render_tool_result(Some(tool_call_id), content, output_mode);
    }
}

/// 判断工具结果是否为重要上下文
pub(super) fn is_important_tool_result(msg: &ChatMessage) -> bool {
    let ChatMessage::Tool { content, .. } = msg else {
        return false;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    let Some(stdout) = val
        .get("result")
        .and_then(|r| r.get("stdout"))
        .and_then(|s| s.as_str())
    else {
        return false;
    };

    crate::context::is_stdout_structural(stdout)
}

pub(super) fn finish_terminal_line(terminal_line_dirty: &mut bool) {
    if *terminal_line_dirty {
        println!();
        *terminal_line_dirty = false;
    }
}

fn render_full_tool_result(result: &serde_json::Value) {
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
}

fn is_process_result(result: &serde_json::Value) -> bool {
    result.get("stdout").is_some()
        || result.get("stderr").is_some()
        || result.get("status").is_some()
        || result.get("success").is_some()
}

fn render_concise_tool_result(wrapper: &serde_json::Value, result: &serde_json::Value) {
    let tool = wrapper["tool"].as_str().unwrap_or("tool");
    let elapsed = wrapper["metrics"]["elapsed_ms"].as_u64().unwrap_or(0);
    let success = result["success"].as_bool().unwrap_or(true);
    let status = result["status"].as_i64();
    let summary = summarize_result(result);

    if success {
        println!(
            "\x1b[32m✓ {}\x1b[0m {}{}",
            tool,
            format_elapsed(elapsed),
            summary
        );
    } else {
        println!(
            "\x1b[31m✗ {}\x1b[0m exit={} {}{}",
            tool,
            status.unwrap_or(-1),
            format_elapsed(elapsed),
            summary
        );
    }
}

fn render_tool_result_json(tool_call_id: Option<&str>, mut value: serde_json::Value) {
    let ok = value["ok"].as_bool().unwrap_or(false);
    let subject = value["tool"].as_str().unwrap_or("tool").to_string();
    let kind = if ok {
        RunEventKind::ToolFinished
    } else {
        RunEventKind::ToolFailed
    };
    if let (Some(id), Some(object)) = (tool_call_id, value.as_object_mut()) {
        object.insert(
            "tool_call_id".to_string(),
            serde_json::Value::String(id.to_string()),
        );
    }
    emit_json_event(kind, &subject, value);
}

fn summarize_result(result: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(status) = result["status"].as_i64() {
        parts.push(format!("exit={}", status));
    }
    if let Some(stdout) = result["stdout"].as_str() {
        if !stdout.is_empty() {
            parts.push(format!(
                "stdout {} lines/{} bytes",
                stdout.lines().count(),
                stdout.len()
            ));
        }
    }
    if let Some(stderr) = result["stderr"].as_str() {
        if !stderr.is_empty() {
            parts.push(format!(
                "stderr {} lines/{} bytes",
                stderr.lines().count(),
                stderr.len()
            ));
        }
    }
    if parts.is_empty() {
        match serde_json::to_string(result) {
            Ok(text) if text.len() <= 120 => parts.push(text),
            Ok(text) => parts.push(format!("{} bytes result", text.len())),
            Err(_) => {}
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("({})", parts.join(", "))
    }
}

fn format_elapsed(elapsed_ms: u64) -> String {
    if elapsed_ms == 0 {
        String::new()
    } else {
        format!("{}ms ", elapsed_ms)
    }
}
