use crate::context::ContextManager;
use crate::session::SessionManager;
use crate::task::TaskManager;

/// ⭐ 处理会话管理命令
pub(super) fn handle_session_command(
    input: &str,
    session_manager: &SessionManager,
    ctx: &mut ContextManager,
    task_manager: &mut TaskManager,
) {
    let trimmed = input.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();

    if parts.is_empty() {
        print_session_help();
        return;
    }

    // /sessions 是 /session list 的快捷方式
    if parts[0] == "/sessions" {
        list_sessions(session_manager);
        return;
    }

    // /session 命令
    if parts.len() < 2 {
        print_session_help();
        return;
    }

    let subcommand = parts[1];

    match subcommand {
        "save" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /session save <名称>\x1b[0m");
                return;
            }
            let name = parts[2..].join(" ");
            match session_manager.save(&name, ctx) {
                Ok(session) => {
                    println!(
                        "\x1b[32m━━━ 💾 会话已保存 ━━━\x1b[0m\n  📁 名称: {}\n  💬 消息数: {}\n  🕐 时间: {}",
                        session.name,
                        session.messages.len(),
                        session.updated_at,
                    );
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 保存失败: {}\x1b[0m", e);
                }
            }
        }
        "load" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /session load <名称>\x1b[0m");
                return;
            }
            let name = parts[2..].join(" ");
            match session_manager.load(&name) {
                Ok(session) => {
                    // 保存当前上下文到自动快照
                    if ctx.get_messages().len() > 1 {
                        let auto_save_name = format!("_autosave_{}", chrono_now_simple());
                        let _ = session_manager.save(&auto_save_name, ctx);
                        println!(
                            "\x1b[90m  💾 当前上下文已自动保存为: {}\x1b[0m",
                            auto_save_name
                        );
                    }

                    // 生成恢复用的系统提示词
                    let restore_prompt = session_manager.default_system_prompt(&session);

                    // 重建 ContextManager
                    let restored_messages =
                        session_manager.restore_messages(&session, &restore_prompt);
                    *ctx = ContextManager::new(restore_prompt, session.strategy.clone());

                    // 恢复消息
                    for msg in restored_messages.into_iter().skip(1) {
                        ctx.add_message(msg);
                    }

                    // 重置任务管理器
                    *task_manager = TaskManager::new(&session.current_dir);
                    task_manager.load();

                    println!(
                        "\x1b[32m━━━ 📂 会话已加载 ━━━\x1b[0m\n  📁 名称: {}\n  💬 消息数: {}\n  🕐 创建: {}\n  🕐 更新: {}",
                        session.name,
                        session.messages.len(),
                        session.created_at,
                        session.updated_at,
                    );
                    println!("\x1b[90m  💡 输入 /session list 查看所有会话\x1b[0m");
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 加载失败: {}\x1b[0m", e);
                    println!("\x1b[33m  💡 使用 /session list 查看可用会话\x1b[0m");
                }
            }
        }
        "list" => {
            list_sessions(session_manager);
        }
        "delete" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /session delete <名称>\x1b[0m");
                return;
            }
            let name = parts[2..].join(" ");
            match session_manager.delete(&name) {
                Ok(true) => {
                    println!("\x1b[32m━━━ 🗑️ 会话已删除: {}\x1b[0m", name);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  会话不存在: {}\x1b[0m", name);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 删除失败: {}\x1b[0m", e);
                }
            }
        }
        "rename" => {
            if parts.len() < 4 {
                println!("\x1b[33m⚠️  用法: /session rename <旧名称> <新名称>\x1b[0m");
                return;
            }
            let old_name = parts[2];
            let new_name = parts[3..].join(" ");
            match session_manager.rename(old_name, &new_name) {
                Ok(true) => {
                    println!(
                        "\x1b[32m━━━ ✏️ 会话已重命名: {} → {}\x1b[0m",
                        old_name, new_name
                    );
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  会话不存在: {}\x1b[0m", old_name);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 重命名失败: {}\x1b[0m", e);
                }
            }
        }
        "help" | "-h" | "--help" => {
            print_session_help();
        }
        other => {
            println!("\x1b[33m⚠️  未知的子命令: {}\x1b[0m", other);
            print_session_help();
        }
    }
}

/// 列出所有会话
fn list_sessions(session_manager: &SessionManager) {
    match session_manager.list() {
        Ok(sessions) => {
            if sessions.is_empty() {
                println!("\x1b[33m📂 暂无保存的会话\x1b[0m");
                println!("\x1b[90m  💡 使用 /session save <名称> 保存当前对话\x1b[0m");
            } else {
                println!(
                    "\x1b[36m━━━ 📂 已保存的会话 (共 {}) ━━━\x1b[0m",
                    sessions.len()
                );
                for session in &sessions {
                    println!("{}", session);
                }
                println!("\x1b[90m  💡 使用 /session load <名称> 恢复对话\x1b[0m");
            }
        }
        Err(e) => {
            println!("\x1b[31m━━━ ❌ 列出会话失败: {}\x1b[0m", e);
        }
    }
}

/// 打印会话管理帮助
fn print_session_help() {
    println!("\x1b[36m━━━ 📋 会话管理命令 ━━━\x1b[0m");
    println!("  \x1b[33m/session save <名称>\x1b[0m    保存当前对话");
    println!("  \x1b[33m/session load <名称>\x1b[0m    加载已保存的对话");
    println!("  \x1b[33m/session list\x1b[0m           列出所有会话");
    println!("  \x1b[33m/session delete <名称>\x1b[0m  删除会话");
    println!("  \x1b[33m/session rename <旧> <新>\x1b[0m  重命名会话");
    println!("  \x1b[33m/sessions\x1b[0m                列出所有会话（快捷方式）");
    println!("  \x1b[33m/session help\x1b[0m            显示此帮助");
}

/// 获取简单的时间字符串（用于自动保存快照命名）
fn chrono_now_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let total_minutes = secs / 60;
    let hours = (total_minutes / 60) % 24;
    let minutes = total_minutes % 60;
    format!("{:02}{:02}", hours, minutes)
}
