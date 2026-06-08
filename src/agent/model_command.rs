use crate::model::ModelManager;

/// ⭐ 处理 /model 命令：模型管理与切换
///
/// 支持子命令：
///   /model list    — 列出所有已注册的模型
///   /model current — 显示当前活跃的模型
///   /model switch <name> — 切换到指定模型
pub(super) fn handle_model_command(input: &str, model_manager: &mut ModelManager) {
    let parts: Vec<&str> = input.trim().split_whitespace().collect();
    if parts.len() < 2 {
        print_model_help();
        return;
    }

    let subcommand = parts[1];
    match subcommand {
        "list" | "ls" => {
            let models = model_manager.list_models();
            let active = model_manager.active_name().to_string();
            println!("\x1b[36m━━━ 🤖 已注册模型 (共 {}) ━━━\x1b[0m", models.len());
            for cfg in &models {
                let indicator = if cfg.name == active { "→ " } else { "  " };
                let active_mark = if cfg.name == active {
                    " \x1b[32m(当前)\x1b[0m"
                } else {
                    ""
                };
                println!(
                    "  {}\x1b[33m{:<12}\x1b[0m {} {}{}",
                    indicator, cfg.name, cfg.model_name, cfg.provider, active_mark,
                );
            }
        }
        "current" | "cur" | "active" => {
            if let Some(cfg) = model_manager.current() {
                println!("\x1b[36m━━━ 🤖 当前活跃模型 ━━━\x1b[0m");
                println!("  \x1b[33m名称:\x1b[0m     {}", cfg.name);
                println!("  \x1b[33m模型:\x1b[0m     {}", cfg.model_name);
                println!("  \x1b[33m提供商:\x1b[0m   {}", cfg.provider);
                println!("  \x1b[33mAPI Base:\x1b[0m {}", cfg.base_url);
            } else {
                println!("\x1b[33m⚠️  当前没有活跃的模型\x1b[0m");
            }
        }
        "switch" | "use" | "set" => {
            if parts.len() < 3 {
                println!("\x1b[33m⚠️  用法: /model switch <模型名称>\x1b[0m");
                return;
            }
            let target = parts[2..].join(" ");
            match model_manager.switch(&target) {
                Ok(_) => {
                    println!(
                        "\x1b[32m━━━ ✅ 已切换到模型 '{}{}' ━━━\x1b[0m",
                        '\'', &target
                    );
                }
                Err(e) => {
                    println!("\x1b[31m━━━ ❌ 切换失败: {}\x1b[0m", e);
                }
            }
        }
        "help" | "-h" | "--help" => {
            print_model_help();
        }
        _ => {
            println!("\x1b[33m⚠️  未知的子命令: {}\x1b[0m", subcommand);
            print_model_help();
        }
    }
}

/// 打印 /model 命令帮助
fn print_model_help() {
    println!("\x1b[36m━━━ 🤖 模型管理命令 ━━━\x1b[0m");
    println!("  \x1b[33m/model list\x1b[0m              列出所有已注册模型");
    println!("  \x1b[33m/model current\x1b[0m           显示当前活跃模型");
    println!("  \x1b[33m/model switch <名称>\x1b[0m      切换到指定模型");
    println!("  \x1b[33m/model help\x1b[0m               显示此帮助");
}
