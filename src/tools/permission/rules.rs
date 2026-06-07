//! 权限规则定义
//!
//! 定义权限沙箱用到的所有规则类型：
//! - 路径规则 (PathRule)：控制 Agent 可以访问的目录
//! - 命令规则 (CommandRule)：控制 Agent 可以执行的命令
//! - 敏感文件规则 (SensitiveFileRule)：保护特定文件免遭读写

use serde::{Deserialize, Serialize};

/// 规则动作：放行或拒绝
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RuleAction {
    Allow,
    Deny,
}

/// 路径规则：控制 Agent 可以操作的目录范围
///
/// 示例：允许项目目录内所有文件
/// ```toml
/// [[path_rules]]
/// action = "Allow"
/// pattern = "/Users/sean/Desktop/repo/agent-lab/**"
/// description = "项目目录"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRule {
    pub action: RuleAction,
    pub pattern: String,
    pub description: String,
}

/// 命令规则：控制 Agent 可以执行的命令
///
/// 支持按命令名和参数约束进行匹配。
/// `args_constraint` 为可选，用于精确匹配特定参数组合。
///
/// 示例：禁止 `rm -rf /`
/// ```toml
/// [[command_rules]]
/// action = "Deny"
/// pattern = "rm"
/// args_constraint = "-rf /"
/// description = "禁止递归删除根目录"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRule {
    pub action: RuleAction,
    pub pattern: String,
    pub args_constraint: Option<String>,
    pub description: String,
}

/// 敏感文件规则：保护特定文件不被读取或写入
///
/// `read_allowed` 和 `write_allowed` 分别控制读写权限。
///
/// 示例：禁止读写 `.env` 文件
/// ```toml
/// [[sensitive_file_rules]]
/// action = "Deny"
/// pattern = "**/.env"
/// read_allowed = false
/// write_allowed = false
/// description = "环境变量文件（含 API Key）"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitiveFileRule {
    pub action: RuleAction,
    pub pattern: String,
    pub read_allowed: bool,
    pub write_allowed: bool,
    pub description: String,
}

/// 完整权限配置
///
/// 包含所有规则和一个项目根目录，所有路径规则都基于此目录进行解析。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionConfig {
    pub version: String,
    pub project_root: String,
    #[serde(default)]
    pub path_rules: Vec<PathRule>,
    #[serde(default)]
    pub command_rules: Vec<CommandRule>,
    #[serde(default)]
    pub sensitive_file_rules: Vec<SensitiveFileRule>,
}

impl PermissionConfig {
    /// 创建默认的权限配置
    ///
    /// 默认配置包含：
    /// - 路径白名单：仅允许项目目录 `/Users/sean/Desktop/repo/agent-lab/**`
    /// - 命令黑名单：禁止 sudo、dd、reboot 等高危命令
    /// - 敏感文件保护：禁止读写 `.env`、`.git/**`、`~/.ssh/**`、`*.pem`
    pub fn default_for_project(project_root: &str) -> Self {
        PermissionConfig {
            version: "1.0".to_string(),
            project_root: project_root.to_string(),
            path_rules: vec![
                PathRule {
                    action: RuleAction::Allow,
                    pattern: format!("{}/**", project_root.trim_end_matches('/')),
                    description: "项目目录".to_string(),
                },
            ],
            command_rules: vec![
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "sudo".to_string(),
                    args_constraint: None,
                    description: "禁止使用 sudo 提升权限".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "dd".to_string(),
                    args_constraint: None,
                    description: "禁止使用 dd 直接读写设备".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "passwd".to_string(),
                    args_constraint: None,
                    description: "禁止修改系统密码".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "chown".to_string(),
                    args_constraint: None,
                    description: "禁止修改文件所有者".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "kill".to_string(),
                    args_constraint: None,
                    description: "禁止杀死进程".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "reboot".to_string(),
                    args_constraint: None,
                    description: "禁止重启系统".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "shutdown".to_string(),
                    args_constraint: None,
                    description: "禁止关机".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "halt".to_string(),
                    args_constraint: None,
                    description: "禁止关机".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "init".to_string(),
                    args_constraint: None,
                    description: "禁止切换系统运行级别".to_string(),
                },
                CommandRule {
                    action: RuleAction::Deny,
                    pattern: "poweroff".to_string(),
                    args_constraint: None,
                    description: "禁止关机".to_string(),
                },
            ],
            sensitive_file_rules: vec![
                SensitiveFileRule {
                    action: RuleAction::Deny,
                    pattern: "**/.env".to_string(),
                    read_allowed: false,
                    write_allowed: false,
                    description: "环境变量文件（含 API Key）".to_string(),
                },
                SensitiveFileRule {
                    action: RuleAction::Deny,
                    pattern: "**/.env.*".to_string(),
                    read_allowed: false,
                    write_allowed: false,
                    description: "环境变量文件变体".to_string(),
                },
                SensitiveFileRule {
                    action: RuleAction::Deny,
                    pattern: "**/.git/**".to_string(),
                    read_allowed: false,
                    write_allowed: false,
                    description: "Git 内部数据".to_string(),
                },
                SensitiveFileRule {
                    action: RuleAction::Deny,
                    pattern: "**/id_rsa*".to_string(),
                    read_allowed: false,
                    write_allowed: false,
                    description: "RSA 私钥文件".to_string(),
                },
                SensitiveFileRule {
                    action: RuleAction::Deny,
                    pattern: "**/*.pem".to_string(),
                    read_allowed: false,
                    write_allowed: false,
                    description: "PEM 密钥文件".to_string(),
                },
                SensitiveFileRule {
                    action: RuleAction::Deny,
                    pattern: "**/*.key".to_string(),
                    read_allowed: false,
                    write_allowed: false,
                    description: "密钥文件".to_string(),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_creation() {
        let config = PermissionConfig::default_for_project("/test/project");
        assert_eq!(config.version, "1.0");
        assert_eq!(config.project_root, "/test/project");
        assert!(!config.path_rules.is_empty());
        assert!(!config.command_rules.is_empty());
        assert!(!config.sensitive_file_rules.is_empty());
    }

    #[test]
    fn test_default_config_has_sudo_denied() {
        let config = PermissionConfig::default_for_project("/test");
        let has_sudo_rule = config.command_rules.iter().any(|r| r.pattern == "sudo");
        assert!(has_sudo_rule, "默认配置应包含 sudo 禁止规则");
    }

    #[test]
    fn test_default_config_has_env_protected() {
        let config = PermissionConfig::default_for_project("/test");
        let has_env_rule = config.sensitive_file_rules.iter().any(|r| r.pattern == "**/.env");
        assert!(has_env_rule, "默认配置应包含 .env 保护规则");
    }
}
