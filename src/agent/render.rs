use crate::model::ChatMessage;

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
pub(super) fn render_tool_result_from_msg(msg: &ChatMessage) {
    if let ChatMessage::Tool { content, .. } = msg {
        render_tool_result(content);
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
