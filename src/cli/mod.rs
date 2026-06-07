// src/commands/mod.rs
//
// ⭐ 命令注册表（CommandRegistry）
//
// 集中管理所有 CLI 斜杠命令（/command），提供：
// - 命令元数据注册（名称、描述、用法、子命令）
// - 帮助信息格式化显示
// - 命令查找与验证
// - 输入自动补全提示

use std::collections::HashMap;

/// ⭐ 命令定义
#[derive(Debug, Clone)]
pub struct Command {
    /// 命令名称（不含前导斜杠）
    pub name: &'static str,
    /// 简短描述（一行）
    pub description: &'static str,
    /// 详细用法说明
    pub usage: &'static str,
    /// 示例列表
    pub examples: &'static [&'static str],
    /// 子命令列表（用于 session 类的复合命令）
    pub subcommands: &'static [Subcommand],
}

/// 子命令结构
#[derive(Debug, Clone)]
pub struct Subcommand {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
}

/// ⭐ 命令注册表
#[derive(Debug)]
pub struct CommandRegistry {
    commands: HashMap<&'static str, &'static Command>,
}

impl CommandRegistry {
    /// 创建注册表并注册所有内置命令
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };
        registry.register_all();
        registry
    }

    fn register_all(&mut self) {
        // 通用命令
        self.register(&Command {
            name: "help",
            description: "显示所有可用命令的帮助信息",
            usage: "/help",
            examples: &["/help"],
            subcommands: &[],
        });

        self.register(&Command {
            name: "clear",
            description: "清空当前对话的历史消息",
            usage: "/clear",
            examples: &["/clear"],
            subcommands: &[],
        });

        // 会话管理命令（主命令）
        self.register(&Command {
            name: "session",
            description: "会话管理：保存、加载、列出、删除、重命名对话",
            usage: "/session <子命令> [参数]",
            examples: &[
                "/session save my-work",
                "/session load my-work",
                "/session list",
                "/session delete my-work",
                "/session rename old new",
            ],
            subcommands: &[
                Subcommand {
                    name: "save",
                    description: "保存当前对话到文件",
                    usage: "/session save <名称>",
                },
                Subcommand {
                    name: "load",
                    description: "加载已保存的对话",
                    usage: "/session load <名称>",
                },
                Subcommand {
                    name: "list",
                    description: "列出所有已保存的会话",
                    usage: "/session list",
                },
                Subcommand {
                    name: "delete",
                    description: "删除指定会话",
                    usage: "/session delete <名称>",
                },
                Subcommand {
                    name: "rename",
                    description: "重命名会话",
                    usage: "/session rename <旧名称> <新名称>",
                },
                Subcommand {
                    name: "help",
                    description: "显示会话管理帮助",
                    usage: "/session help",
                },
            ],
        });

        self.register(&Command {
            name: "sessions",
            description: "列出所有已保存的会话（/session list 的快捷方式）",
            usage: "/sessions",
            examples: &["/sessions"],
            subcommands: &[],
        });

        self.register(&Command {
            name: "tools",
            description: "列出所有可用工具及其描述",
            usage: "/tools",
            examples: &["/tools"],
            subcommands: &[],
        });
    }

    fn register(&mut self, cmd: &'static Command) {
        self.commands.insert(cmd.name, cmd);
    }

    /// 根据命令名称查找命令
    pub fn get(&self, name: &str) -> Option<&&Command> {
        self.commands.get(name)
    }

    /// 检查是否是已知的 / 命令
    pub fn is_known(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    /// 获取所有已注册的命令（按名称排序）
    pub fn all_commands(&self) -> Vec<&&Command> {
        let mut cmds: Vec<&&Command> = self.commands.values().collect();
        cmds.sort_by_key(|c| c.name);
        cmds
    }

    /// ⭐ 显示所有命令的帮助（简短列表）
    pub fn print_help_short(&self) {
        println!("{}", self.format_help_short());
    }

    pub fn format_help_short(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("\x1b[36m━━━ 📋 可用命令 ━━━\x1b[0m\n"));

        for cmd in self.all_commands() {
            let usage_colored = format!("\x1b[33m/{}\x1b[0m", cmd.usage);
            output.push_str(&format!("  {:<30} {}\n", usage_colored, cmd.description));
        }

        output.push_str(&format!("\x1b[90m  💡 输入 /help 查看详细帮助\x1b[0m\n"));
        output
    }

    /// ⭐ 显示某个命令的详细帮助
    pub fn print_command_help(&self, cmd: &Command) {
        println!("{}", self.format_command_help(cmd));
    }

    pub fn format_command_help(&self, cmd: &Command) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "\x1b[36m━━━ 📖 命令: /\x1b[33m{}\x1b[36m ━━━\x1b[0m\n",
            cmd.name
        ));
        output.push_str(&format!("  {}\n", cmd.description));
        output.push_str(&format!("  \x1b[90m用法:\x1b[0m \x1b[33m{}\x1b[0m\n", cmd.usage));

        if !cmd.examples.is_empty() {
            output.push_str(&format!("  \x1b[90m示例:\x1b[0m\n"));
            for example in cmd.examples {
                output.push_str(&format!("    \x1b[33m{}\x1b[0m\n", example));
            }
        }

        if !cmd.subcommands.is_empty() {
            output.push_str(&format!("  \x1b[90m子命令:\x1b[0m\n"));
            for sub in cmd.subcommands {
                output.push_str(&format!(
                    "    \x1b[33m{:<25}\x1b[0m {}\n",
                    sub.usage,
                    sub.description
                ));
            }
        }

        output
    }

    /// ⭐ 显示完整帮助（所有命令的详细帮助）
    pub fn print_help_full(&self) {
        println!("{}", self.format_help_full());
    }

    pub fn format_help_full(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("\x1b[36m━━━ 📖 完整帮助 ━━━\x1b[0m\n"));
        output.push_str(&format!(
            "\x1b[90m所有以 / 开头的输入都会被视为命令。\n输入 /<命令> 执行对应命令。\x1b[0m\n\n"
        ));

        for cmd in self.all_commands() {
            output.push_str(&format!("{}\n", self.format_command_help(cmd)));
        }

        output
    }

    /// ⭐ 显示未知命令提示
    pub fn print_unknown_command(&self, input: &str) {
        let cmd_name = input.trim_start_matches('/').split_whitespace().next().unwrap_or("");
        println!(
            "\x1b[33m⚠️  未知命令: /\x1b[31m{}\x1b[0m",
            cmd_name
        );
        println!("\x1b[90m  输入 /help 查看所有可用命令\x1b[0m");
        println!();
        self.print_help_short();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_builtin_commands() {
        let registry = CommandRegistry::new();
        assert!(registry.is_known("help"));
        assert!(registry.is_known("clear"));
        assert!(registry.is_known("session"));
        assert!(registry.is_known("sessions"));
        assert!(registry.is_known("tools"));
    }

    #[test]
    fn test_registry_unknown_command() {
        let registry = CommandRegistry::new();
        assert!(!registry.is_known("xyz"));
        assert!(!registry.is_known("foobar"));
    }

    #[test]
    fn test_get_command_metadata() {
        let registry = CommandRegistry::new();
        let help_cmd = registry.get("help").unwrap();
        assert_eq!(help_cmd.name, "help");
        assert!(!help_cmd.description.is_empty());

        let session_cmd = registry.get("session").unwrap();
        assert_eq!(session_cmd.name, "session");
        assert!(session_cmd.subcommands.len() > 0);
    }

    #[test]
    fn test_format_help_short_contains_commands() {
        let registry = CommandRegistry::new();
        let help = registry.format_help_short();
        assert!(help.contains("/help"));
        assert!(help.contains("/clear"));
        assert!(help.contains("/session"));
    }

    #[test]
    fn test_all_commands_sorted() {
        let registry = CommandRegistry::new();
        let all = registry.all_commands();
        for i in 1..all.len() {
            assert!(all[i-1].name <= all[i].name, "commands should be sorted by name");
        }
    }
}
