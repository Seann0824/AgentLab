use crate::goal::GoalRegistry;

/// ⭐ 处理 /goal 命令：目标管理
///
/// 支持子命令：
///   /goal list                         — 列出所有目标
///   /goal set <描述>                   — 设置新目标
///   /goal complete <id>                — 标记目标为已完成
///   /goal fail <id> [原因]             — 标记目标为失败
///   /goal cancel <id>                  — 取消目标
///   /goal status                       — 显示当前活跃目标
///   /goal history                      — 显示历史目标
pub(super) fn handle_goal_command(input: &str, goal_manager: &mut GoalRegistry) {
    let parts: Vec<&str> = input.trim().split_whitespace().collect();
    if parts.len() < 2 {
        print_goal_help();
        return;
    }

    let subcommand = parts[1];
    match subcommand {
        "list" | "ls" => {
            let goals = goal_manager.list();
            println!("\x1b[36m━━━ 🎯 所有目标 (共 {}) ━━━\x1b[0m", goals.len());
            for goal in &goals {
                let status_str = goal.status.to_string();
                let status_icon = match status_str.to_lowercase().as_str() {
                    "active" => "\x1b[32m🟢\x1b[0m",
                    "completed" => "\x1b[34m✅\x1b[0m",
                    "failed" => "\x1b[31m❌\x1b[0m",
                    "cancelled" => "\x1b[33m🚫\x1b[0m",
                    _ => "\x1b[90m⚪\x1b[0m",
                };
                println!(
                    "  {} \x1b[33m{:<8}\x1b[0m {}",
                    status_icon, goal.id, goal.description
                );
            }
        }

        "set" | "add" | "new" | "create" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal set <目标描述>\x1b[0m");
                return;
            }
            let description = parts[2..].join(" ");
            match goal_manager.create_goal(description.clone()) {
                Ok(goal) => {
                    println!(
                        "\x1b[32m━━━ 🎯 新目标已创建 ━━━\x1b[0m\n  🆔 ID: \x1b[33m{}\x1b[0m\n  📝 描述: {}\n  📂 状态: \x1b[32mactive\x1b[0m",
                        goal.id, goal.description
                    );
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 创建目标失败: {}\x1b[0m", e);
                }
            }
        }
        "complete" | "done" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal complete <id>\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            match goal_manager.mark_complete(goal_id) {
                Ok(true) => {
                    println!("\x1b[32m━━━ ✅ 目标 '{}' 已完成 ━━━\x1b[0m 🎉", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 标记完成失败: {}\x1b[0m", e);
                }
            }
        }
        "fail" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal fail <id> [原因]\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            let reason = if parts.len() > 3 {
                parts[3..].join(" ")
            } else {
                "unexpected error".to_string()
            };
            match goal_manager.mark_failed(goal_id, &reason) {
                Ok(true) => {
                    println!("\x1b[31m━━━ ❌ 目标 '{}' 已标记为失败 ━━━\x1b[0m", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 标记失败失败: {}\x1b[0m", e);
                }
            }
        }
        "cancel" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal cancel <id>\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            match goal_manager.mark_cancelled(goal_id) {
                Ok(true) => {
                    println!("\x1b[33m━━━ 🚫 目标 '{}' 已取消 ━━━\x1b[0m", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 取消失败: {}\x1b[0m", e);
                }
            }
        }
        "status" | "cur" | "current" | "active" => {
            if let Some(goal) = goal_manager.active_goal() {
                println!("\x1b[36m━━━ 🎯 当前活跃目标 ━━━\x1b[0m");
                println!("  🆔 ID:     \x1b[33m{}\x1b[0m", goal.id);
                println!("  📝 描述:   {}", goal.description);
                println!("  📂 状态:   \x1b[32m{}\x1b[0m", goal.status.to_string());
                println!("  🕐 创建:   {}", goal.created_at);
            } else {
                println!("\x1b[33m⚠️  当前没有活跃目标\x1b[0m");
                println!("\x1b[90m  💡 使用 /goal set <描述> 创建新目标\x1b[0m");
            }
        }
        "history" | "hist" | "log" => {
            let goals = goal_manager.list();
            let completed: Vec<_> = goals
                .iter()
                .filter(|g| g.status.to_string().to_lowercase() != "active")
                .collect();
            if completed.is_empty() {
                println!("\x1b[33m📜 暂无历史目标记录\x1b[0m");
            } else {
                println!(
                    "\x1b[36m━━━ 📜 历史目标 (共 {}) ━━━\x1b[0m",
                    completed.len()
                );
                for goal in completed {
                    let status_str = goal.status.to_string();
                    let status_icon = match status_str.to_lowercase().as_str() {
                        "completed" => "\x1b[34m✅\x1b[0m",
                        "failed" => "\x1b[31m❌\x1b[0m",
                        "cancelled" => "\x1b[33m🚫\x1b[0m",
                        _ => "\x1b[90m⚪\x1b[0m",
                    };
                    println!(
                        "  {} \x1b[33m{:<8}\x1b[0m {}",
                        status_icon, goal.id, goal.description,
                    );
                }
            }
        }
        "delete" | "rm" | "remove" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /goal delete <id>\x1b[0m");
                return;
            }
            let goal_id = parts[2];
            match goal_manager.delete(goal_id) {
                Ok(true) => {
                    println!("\x1b[32m━━━ 🗑️ 目标 '{}' 已删除 ━━━\x1b[0m", goal_id);
                }
                Ok(false) => {
                    println!("\x1b[33m⚠️  找不到目标: {}\x1b[0m", goal_id);
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 删除失败: {}\x1b[0m", e);
                }
            }
        }
        "clear" | "clean" | "purge" => {
            let count = goal_manager.list().len();
            if count == 0 {
                println!("\x1b[33m⚠️  当前没有目标需要清理\x1b[0m");
                return;
            }
            // 要求确认
            if parts.len() > 2 && (parts[2] == "--force" || parts[2] == "-f") {
                match goal_manager.clear_all() {
                    Ok(n) => {
                        println!("\x1b[32m━━━ 🧹 已清空 {} 个目标 ━━━\x1b[0m", n);
                    }
                    Err(e) => {
                        println!("\x1b[31m━━━ ❌ 清空失败: {}\x1b[0m", e);
                    }
                }
            } else {
                println!("\x1b[33m⚠️  确定要清空所有 {} 个目标吗？\x1b[0m", count);
                println!("  \x1b[33m使用 /goal clear --force 确认执行\x1b[0m");
            }
        }
        "help" | "-h" | "--help" => {
            print_goal_help();
        }
        _ => {
            println!("\x1b[33m⚠️  未知的子命令: {}\x1b[0m", subcommand);
            print_goal_help();
        }
    }
}

/// 打印 /goal 命令帮助
fn print_goal_help() {
    println!("\x1b[36m━━━ 🎯 目标管理命令 ━━━\x1b[0m");
    println!(
        "  \x1b[33m/goal set <描述>\x1b[0m          创建新目标（设定后 agent 会持续推进直到完成）"
    );
    println!("  \x1b[33m/goal list\x1b[0m                列出所有目标");
    println!("  \x1b[33m/goal status\x1b[0m              显示当前活跃目标");
    println!("  \x1b[33m/goal complete <id>\x1b[0m       标记目标为已完成");
    println!("  \x1b[33m/goal fail <id> [原因]\x1b[0m    标记目标为失败");
    println!("  \x1b[33m/goal cancel <id>\x1b[0m         取消目标");
    println!("  \x1b[33m/goal delete <id>\x1b[0m         删除目标（从磁盘彻底移除）");
    println!("  \x1b[33m/goal clear --force\x1b[0m       清空所有目标（需确认）");
    println!("  \x1b[33m/goal history\x1b[0m             查看历史目标");
    println!("  \x1b[33m/goal help\x1b[0m                显示此帮助");
}
