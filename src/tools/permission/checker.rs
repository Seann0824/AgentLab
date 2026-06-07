//! 权限检查引擎
//!
//! 核心职责：
//! 1. 解析 Shell 命令，提取结构化信息（命令名、参数、路径、URL）
//! 2. 对命令执行前置检查（pre_check），判断是否违反权限规则
//! 3. 返回结构化的检查结果，包含拒绝原因和建议替代方案
//!
//! 检查流程：
//! ```text
//! 原始命令 → 词法解析 → 命令黑名单检查 → sudo检查 → 路径白名单检查
//!                                                   ↓
//!                                           敏感文件检查 → 网络检查 → 通过
//! ```
//!
//! 设计原则：默认安全（Deny by default），只放行明确允许的操作。

use std::path::Path;

use super::rules::{CommandRule, PathRule, PermissionConfig, RuleAction, SensitiveFileRule};

// ─── 检查结果类型 ─────────────────────────────────────────────

/// 权限检查结果
#[derive(Debug, Clone)]
pub enum CheckResult {
    /// 通过检查，可以执行
    Allowed {
        rule: String,
    },
    /// 被规则拒绝
    Denied {
        code: String,
        rule: String,
        message: String,
        suggestion: String,
    },
}

impl CheckResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, CheckResult::Allowed { .. })
    }

    /// 转换为返回给模型的错误 JSON
    pub fn to_error_json(&self) -> serde_json::Value {
        match self {
            CheckResult::Denied { code, rule, message, suggestion } => {
                serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": code,
                        "rule": rule,
                        "message": message,
                        "suggestion": suggestion,
                    }
                })
            }
            CheckResult::Allowed { .. } => {
                serde_json::json!({
                    "ok": true,
                    "result": "permission check passed"
                })
            }
        }
    }
}

// ─── 命令解析 ─────────────────────────────────────────────────

/// 解析后的 Shell 命令结构化信息
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    /// 原始命令字符串
    pub raw: String,
    /// 主命令名称（如 "cat", "rm", "curl"）
    pub command: String,
    /// 参数列表
    pub args: Vec<String>,
    /// 涉及的文件路径（从参数中提取，不以 - 开头且包含 / 或 .）
    pub file_paths: Vec<String>,
    /// 涉及的 URL（从参数中提取，以 http:// 或 https:// 开头）
    pub urls: Vec<String>,
    /// 是否包含 sudo
    pub has_sudo: bool,
    /// 是否包含管道或重定向
    pub has_pipe_or_redirect: bool,
}

impl ParsedCommand {
    /// 从原始命令字符串解析
    ///
    /// 处理单引号、双引号内的空格，不支持复杂的 Shell 语法（如 $()、`` 等）。
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let parts = Self::split_shell_words(trimmed);

        let has_sudo = parts.first().map(|s| s.as_str() == "sudo").unwrap_or(false);

        let command = if has_sudo {
            parts.get(1).cloned().unwrap_or_default()
        } else {
            parts.first().cloned().unwrap_or_default()
        };

        let args: Vec<String> = parts
            .iter()
            .skip(if has_sudo { 2 } else { 1 })
            .cloned()
            .collect();

        // 提取文件路径：不以 - 开头，且包含 / 或 . 的参数字段
        let file_paths: Vec<String> = args
            .iter()
            .filter(|a| {
                !a.starts_with('-')
                    && (a.contains('/') || a.contains('\\') || a.starts_with('.'))
            })
            .cloned()
            .collect();

        // 提取 URL
        let urls: Vec<String> = args
            .iter()
            .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
            .cloned()
            .collect();

        let has_pipe_or_redirect =
            trimmed.contains('|') || trimmed.contains('>') || trimmed.contains('<');

        ParsedCommand {
            raw: raw.to_string(),
            command,
            args,
            file_paths,
            urls,
            has_sudo,
            has_pipe_or_redirect,
        }
    }

    /// 简单的 Shell 命令分割器
    ///
    /// 处理：
    /// - 空格分割单词
    /// - 单引号 `'...'`：内部所有字符保持原样
    /// - 双引号 `"..."`：内部字符保持原样（不支持转义）
    /// - 连续空格合并
    fn split_shell_words(input: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let ch = chars[i];
            match ch {
                '\'' if !in_double_quote => {
                    in_single_quote = !in_single_quote;
                }
                '"' if !in_single_quote => {
                    in_double_quote = !in_double_quote;
                }
                ' ' | '\t' if !in_single_quote && !in_double_quote => {
                    if !current.is_empty() {
                        words.push(current.clone());
                        current.clear();
                    }
                }
                _ => {
                    current.push(ch);
                }
            }
            i += 1;
        }

        // 最后一个单词
        if !current.is_empty() {
            words.push(current);
        }

        words
    }
}

// ─── Glob 匹配 ─────────────────────────────────────────────────
// ─── Glob 匹配 ─────────────────────────────────────────────────

/// 简单的 Glob 模式匹配
///
/// 支持的通配符：
/// - `*`：匹配任意字符（不包括路径分隔符 `/`）
/// - `**`：匹配任意字符（包括路径分隔符 `/`）
/// - `?`：匹配单个字符（不包括路径分隔符 `/`）
///
/// 不支持：`[...]` 字符类、`{...}` 选择组
fn glob_match(pattern: &str, path: &str) -> bool {
    // 没有通配符时直接比较
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern.trim_end_matches('/') == path.trim_end_matches('/');
    }
    
    // 没有 ** 时使用 simple_glob_match
    if !pattern.contains("**") {
        return simple_glob_match(pattern, path);
    }
    
    // 有 **：将 pattern 按 ** 拆分，每段用 simple_glob_match 匹配
    // 第一段必须匹配路径开头，最后一段必须匹配路径结尾
    // 中间段可以在任意位置出现
    let parts: Vec<&str> = pattern.split("**").collect();
    let mut path_remaining = path;
    let n = parts.len();
    
    for (i, part) in parts.iter().enumerate() {
        let part = if i == 0 {
            // 第一段保留前导 /
            part.trim_end_matches('/')
        } else {
            part.trim_matches('/')
        };
        if part.is_empty() {
            continue;
        }
        
        if i == 0 {
            // 第一段：必须匹配开头
            if let Some(rest) = match_prefix(part, path_remaining) {
                path_remaining = rest;
            } else {
                return false;
            }
        } else if i == n - 1 {
            // 最后一段：必须匹配结尾
            return match_suffix(part, path_remaining);
        } else {
            // 中间段：在路径任意位置匹配
            if let Some(rest) = match_anywhere(part, path_remaining) {
                path_remaining = rest;
            } else {
                return false;
            }
        }
    }
    
    true
}

/// 从路径开头匹配 pattern，返回匹配后的剩余部分
fn match_prefix<'a>(pattern: &str, path: &'a str) -> Option<&'a str> {
    let p_len = pattern.len();
    let s_len = path.len();
    
    // 尝试从不同长度匹配
    for end in p_len..=s_len {
        let candidate = &path[..end];
        // candidate 要么完全匹配 pattern，要么 pattern 最后是 * 可以匹配更多
        if simple_glob_match(pattern, candidate) {
            return Some(&path[end..]);
        }
    }
    None
}

/// 在路径中查找匹配 pattern 的子串，返回匹配后的剩余部分
fn match_anywhere<'a>(pattern: &str, path: &'a str) -> Option<&'a str> {
    for start in 0..=path.len() {
        for end in start..=path.len() {
            let candidate = &path[start..end];
            if simple_glob_match(pattern, candidate) {
                return Some(&path[end..]);
            }
        }
    }
    None
}

/// 检查路径末尾是否匹配 pattern
fn match_suffix(pattern: &str, path: &str) -> bool {
    for start in 0..=path.len() {
        let candidate = &path[start..];
        if simple_glob_match(pattern, candidate) {
            return true;
        }
    }
    false
}

/// 简单 Glob 匹配（无 `**`）
fn simple_glob_match(pattern: &str, path: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let s_chars: Vec<char> = path.chars().collect();
    let (m, n) = (p_chars.len(), s_chars.len());

    // DP: dp[i][j] = pattern[..i] 是否匹配 path[..j]
    let mut dp = vec![vec![false; n + 1]; m + 1];
    dp[0][0] = true;

    // 处理 pattern 开头的 *
    for i in 1..=m {
        if p_chars[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }

    for i in 1..=m {
        for j in 1..=n {
            match p_chars[i - 1] {
                '?' => {
                    if s_chars[j - 1] != '/' {
                        dp[i][j] = dp[i - 1][j - 1];
                    }
                }
                '*' => {
                    // * 不匹配 /
                    if s_chars[j - 1] == '/' {
                        dp[i][j] = dp[i][j - 1];
                    } else {
                        dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
                    }
                }
                c => {
                    dp[i][j] = dp[i - 1][j - 1] && c == s_chars[j - 1];
                }
            }
        }
    }

    dp[m][n]
}
// ─── 权限检查引擎 ─────────────────────────────────────────────

/// 权限检查引擎
///
/// 负责对 Shell 命令进行多层权限检查：
/// 1. 命令黑名单检查
/// 2. sudo 检查
/// 3. 敏感文件检查
/// 4. 路径白名单检查
/// 5. 网络请求检查
pub struct PermissionChecker {
    config: PermissionConfig,
}

impl PermissionChecker {
    /// 使用指定配置创建检查器
    pub fn new(config: PermissionConfig) -> Self {
        Self { config }
    }

    /// 使用项目根目录的默认配置创建检查器
    pub fn default_for_project(project_root: &str) -> Self {
        Self {
            config: PermissionConfig::default_for_project(project_root),
        }
    }

    /// 获取当前配置的引用
    pub fn config(&self) -> &PermissionConfig {
        &self.config
    }

    /// 前置检查：在命令执行前调用
    ///
    /// 检查顺序：
    /// 1. 命令黑名单 — 高危命令直接拒绝
    /// 2. Sudo 检查 — 禁止提权
    /// 3. 敏感文件 — 保护 .env、密钥文件等
    /// 4. 路径白名单 — 只能在项目目录内操作
    /// 5. 网络请求 — 默认禁止外部网络访问
    pub fn pre_check(&self, raw_command: &str) -> CheckResult {
        let parsed = ParsedCommand::parse(raw_command);

        // 空命令
        if parsed.command.is_empty() {
            return CheckResult::Denied {
                code: "empty_command".to_string(),
                rule: "基本检查".to_string(),
                message: "命令不能为空".to_string(),
                suggestion: "请提供要执行的命令".to_string(),
            };
        }

        // 1. 命令黑名单检查
        for rule in &self.config.command_rules {
            if self.matches_command(&parsed, rule) {
                match rule.action {
                    RuleAction::Deny => {
                        let suggestion = self.suggest_alternative(&parsed.command);
                        return CheckResult::Denied {
                            code: "command_denied".to_string(),
                            rule: rule.description.clone(),
                            message: format!(
                                "命令 '{}' 被安全规则禁止: {}",
                                parsed.command, rule.description
                            ),
                            suggestion,
                        };
                    }
                    RuleAction::Allow => {
                        // 明确放行，跳过后续命令检查
                        break;
                    }
                }
            }
        }

        // 2. Sudo 检查
        if parsed.has_sudo {
            return CheckResult::Denied {
                code: "sudo_denied".to_string(),
                rule: "禁止使用 sudo".to_string(),
                message: "Agent 不允许使用 sudo 执行命令，这会影响系统安全。".to_string(),
                suggestion: "移除 sudo，使用普通权限执行".to_string(),
            };
        }

        // 3. 路径检查（敏感文件 + 白名单）
        for file_path in &parsed.file_paths {
            let abs_path = self.resolve_absolute_path(file_path);

            // 3a. 敏感文件检查
            for rule in &self.config.sensitive_file_rules {
                if self.matches_glob(&rule.pattern, &abs_path) {
                    let is_write = self.is_write_operation(&parsed.command);
                    if (is_write && !rule.write_allowed) || (!is_write && !rule.read_allowed) {
                        return CheckResult::Denied {
                            code: "sensitive_file_denied".to_string(),
                            rule: rule.description.clone(),
                            message: format!(
                                "访问被安全规则禁止: {} {}",
                                if is_write { "写入" } else { "读取" },
                                file_path
                            ),
                            suggestion: format!(
                                "文件 '{}' 受保护（{}），不允许{}",
                                file_path,
                                rule.description,
                                if is_write { "写入" } else { "读取" }
                            ),
                        };
                    }
                }
            }

            // 3b. 路径白名单检查
            let in_allowed = self.config.path_rules.iter().any(|r| {
                r.action == RuleAction::Allow && self.matches_glob(&r.pattern, &abs_path)
            });

            if !in_allowed {
                return CheckResult::Denied {
                    code: "path_not_allowed".to_string(),
                    rule: "路径白名单".to_string(),
                    message: format!("路径不在允许的操作范围内: {}", file_path),
                    suggestion: format!(
                        "请在项目目录 '{}' 内操作",
                        self.config.project_root
                    ),
                };
            }
        }

        // 4. 网络请求检查
        for url in &parsed.urls {
            if !self.is_url_allowed(url) {
                return CheckResult::Denied {
                    code: "network_denied".to_string(),
                    rule: "网络访问限制".to_string(),
                    message: format!("不允许网络请求到: {}", url),
                    suggestion: "网络请求默认禁止，如需使用请在配置中放行".to_string(),
                };
            }
        }

        CheckResult::Allowed {
            rule: "所有检查通过".to_string(),
        }
    }

    /// 判断是否是写操作命令
    fn is_write_operation(&self, command: &str) -> bool {
        matches!(
            command,
            "rm"
                | "mv"
                | "cp"
                | "dd"
                | "chmod"
                | "chown"
                | "touch"
                | "mkdir"
                | "ln"
                | "truncate"
                | "install"
        )
    }

    /// 解析为绝对路径
    fn resolve_absolute_path(&self, file_path: &str) -> String {
        let base = Path::new(&self.config.project_root);

        // 处理 ~/ 开头的路径
        let expanded = if file_path.starts_with("~/") {
            if let Some(home) = std::env::var("HOME").ok() {
                file_path.replacen('~', &home, 1)
            } else {
                file_path.to_string()
            }
        } else {
            file_path.to_string()
        };

        let target = if expanded.starts_with('/') {
            Path::new(&expanded).to_path_buf()
        } else {
            base.join(&expanded)
        };

        // 尝试规范化路径（如果文件存在）
        if target.exists() {
            target
                .canonicalize()
                .unwrap_or(target)
                .to_string_lossy()
                .to_string()
        } else {
            // 如果文件不存在，做逻辑规范化
            let normalized = self.logical_normalize(&target);
            normalized.to_string_lossy().to_string()
        }
    }

    /// 逻辑规范化路径（不依赖文件系统）
    fn logical_normalize(&self, path: &Path) -> std::path::PathBuf {
        use std::path::Component;

        let mut components = Vec::new();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    components.pop();
                }
                Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                    components.push(component);
                }
                Component::CurDir => {
                    // 跳过 .
                }
            }
        }

        let mut result = std::path::PathBuf::new();
        for comp in components {
            result.push(comp);
        }
        result
    }

    /// Glob 模式匹配
    fn matches_glob(&self, pattern: &str, path: &str) -> bool {
        // 对于 **/.env 这类模式，提取文件名部分做匹配
        if pattern.starts_with("**/") {
            let suffix = &pattern[3..];
            // 检查路径是否以 suffix 结尾或者路径的某部分匹配
            if path.ends_with(suffix) {
                return true;
            }
            // 也检查路径的每个段
            if let Some(file_name) = Path::new(path).file_name() {
                if file_name == suffix {
                    return true;
                }
            }
            return glob_match(pattern, path);
        }
        glob_match(pattern, path)
    }

    /// 检查命令是否匹配规则
    fn matches_command(&self, parsed: &ParsedCommand, rule: &CommandRule) -> bool {
        // 命令名匹配
        let name_match = self.simple_string_match(&rule.pattern, &parsed.command);

        if !name_match {
            return false;
        }

        // 如果有参数约束，检查参数
        if let Some(constraint) = &rule.args_constraint {
            let args_str = parsed.args.join(" ");
            return args_str.contains(constraint);
        }

        true
    }

    /// 简单字符串匹配（支持 * 通配符）
    fn simple_string_match(&self, pattern: &str, value: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                return value.starts_with(parts[0]) && value.ends_with(parts[1]);
            }
            return value == pattern;
        }
        value == pattern
    }

    /// 检查 URL 是否允许访问
    fn is_url_allowed(&self, _url: &str) -> bool {
        // 默认禁止所有外部网络请求
        false
    }

    /// 为被拒命令提供替代建议
    fn suggest_alternative(&self, command: &str) -> String {
        match command {
            "sudo" => "移除 sudo，使用普通用户权限执行".to_string(),
            "rm" => "使用前请确认路径是否正确，考虑先 ls 查看".to_string(),
            "dd" => "禁止使用 dd 命令，它可以直接读写底层设备".to_string(),
            "curl" | "wget" => "网络请求默认禁止，如需使用请配置放行规则".to_string(),
            "chmod" => "谨慎修改文件权限，确保不会导致安全问题".to_string(),
            "chown" => "禁止修改文件所有者".to_string(),
            "kill" => "禁止杀死进程".to_string(),
            "reboot" | "shutdown" | "halt" | "poweroff" | "init" => {
                "禁止执行系统管理命令".to_string()
            }
            _ => "此命令被安全规则禁止，请检查是否必要，或联系管理员放行".to_string(),
        }
    }

    /// 生成权限规则的文本摘要，用于注入系统提示词
    ///
    /// 让 AI 模型提前知道权限边界，避免反复尝试被拒操作。
    pub fn generate_policy_summary(&self) -> String {
        use super::rules::RuleAction;

        let mut summary = String::from("## ⚠️ 安全权限规则\n\n");

        summary.push_str("### 允许的操作路径\n");
        for rule in &self.config.path_rules {
            if rule.action == RuleAction::Allow {
                summary.push_str(&format!(
                    "- ✅ `{}` — {}\n",
                    rule.pattern, rule.description
                ));
            }
        }

        summary.push_str("\n### 禁止的命令\n");
        for rule in &self.config.command_rules {
            if rule.action == RuleAction::Deny {
                let constraint = rule
                    .args_constraint
                    .as_ref()
                    .map(|c| format!(" (参数: {})", c))
                    .unwrap_or_default();
                summary.push_str(&format!(
                    "- 🚫 `{}`{} — {}\n",
                    rule.pattern, constraint, rule.description
                ));
            }
        }

        summary.push_str("\n### 受保护的文件\n");
        for rule in &self.config.sensitive_file_rules {
            if rule.action == RuleAction::Deny {
                summary.push_str(&format!(
                    "- 🔒 `{}` — {}（读取: {}, 写入: {}）\n",
                    rule.pattern,
                    rule.description,
                    if rule.read_allowed { "✅" } else { "❌" },
                    if rule.write_allowed { "✅" } else { "❌" }
                ));
            }
        }

        summary.push_str(
            "\n> 如果某个操作被拒绝，请查看返回错误中的 `suggestion` 字段寻求替代方案。\n",
        );

        summary
    }
}

// ─── 测试 ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::permission::rules::PermissionConfig;

    fn test_checker() -> PermissionChecker {
        let config = PermissionConfig {
            version: "test".to_string(),
            project_root: "/test/project".to_string(),
            path_rules: vec![
                super::super::rules::PathRule {
                    action: RuleAction::Allow,
                    pattern: "/test/project/**".to_string(),
                    description: "测试项目目录".to_string(),
                },
            ],
            command_rules: vec![
                super::super::rules::CommandRule {
                    action: RuleAction::Deny,
                    pattern: "sudo".to_string(),
                    args_constraint: None,
                    description: "禁止 sudo".to_string(),
                },
                super::super::rules::CommandRule {
                    action: RuleAction::Deny,
                    pattern: "rm".to_string(),
                    args_constraint: Some("-rf /".to_string()),
                    description: "禁止 rm -rf /".to_string(),
                },
                super::super::rules::CommandRule {
                    action: RuleAction::Deny,
                    pattern: "dd".to_string(),
                    args_constraint: None,
                    description: "禁止 dd".to_string(),
                },
            ],
            sensitive_file_rules: vec![
                super::super::rules::SensitiveFileRule {
                    action: RuleAction::Deny,
                    pattern: "**/.env".to_string(),
                    read_allowed: false,
                    write_allowed: false,
                    description: "环境变量文件".to_string(),
                },
            ],
        };
        PermissionChecker::new(config)
    }

    // ─── 命令解析测试 ──────────────────────────────────

    #[test]
    fn test_parse_simple_command() {
        let parsed = ParsedCommand::parse("ls -la /tmp");
        assert_eq!(parsed.command, "ls");
        assert_eq!(parsed.args, vec!["-la", "/tmp"]);
        assert!(!parsed.has_sudo);
    }

    #[test]
    fn test_parse_with_sudo() {
        let parsed = ParsedCommand::parse("sudo rm -rf /");
        assert_eq!(parsed.command, "rm");
        assert!(parsed.has_sudo);
    }

    #[test]
    fn test_parse_with_quotes() {
        let parsed = ParsedCommand::parse("echo 'hello world'");
        assert_eq!(parsed.command, "echo");
        assert_eq!(parsed.args, vec!["hello world"]);
    }

    #[test]
    fn test_parse_with_double_quotes() {
        let parsed = ParsedCommand::parse("echo \"hello world\"");
        assert_eq!(parsed.command, "echo");
        assert_eq!(parsed.args, vec!["hello world"]);
    }

    #[test]
    fn test_parse_url_extraction() {
        let parsed = ParsedCommand::parse("curl https://example.com/api");
        assert_eq!(parsed.urls, vec!["https://example.com/api"]);
    }

    #[test]
    fn test_parse_empty() {
        let parsed = ParsedCommand::parse("");
        assert_eq!(parsed.command, "");
    }

    // ─── Glob 匹配测试 ──────────────────────────────────

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_match("hello.txt", "hello.txt"));
    }

    #[test]
    fn test_glob_wildcard() {
        assert!(glob_match("*.txt", "hello.txt"));
        assert!(!glob_match("*.txt", "hello.md"));
    }

    #[test]
    fn test_glob_double_star() {
        assert!(glob_match("**/.env", "/test/project/.env"));
        assert!(glob_match("**/.env", "/a/b/c/.env"));
    }

    #[test]
    fn test_glob_double_star_dir() {
        assert!(glob_match("**/.git/**", "/test/project/.git/config"));
        assert!(glob_match("**/.git/**", "/a/b/.git/objects/abc123"));
    }

    #[test]
    fn test_glob_not_match() {
        assert!(!glob_match("*.rs", "hello.md"));
    }

    // ─── 权限检查测试 ──────────────────────────────────

    #[test]
    fn test_allow_safe_command() {
        let checker = test_checker();
        let result = checker.pre_check("ls -la");
        assert!(result.is_allowed(), "安全命令应放行");
    }

    #[test]
    fn test_deny_sudo() {
        let checker = test_checker();
        // "sudo rm -rf /" 会被 rm 命令规则拦截（因为 command = "rm"）
        let result = checker.pre_check("sudo rm -rf /");
        assert!(!result.is_allowed(), "sudo 命令应被拒绝");
        // 同时测试 "sudo ls" 会被 sudo 规则拦截
        let result2 = checker.pre_check("sudo ls");
        assert!(!result2.is_allowed(), "纯 sudo 命令应被拒绝");
        if let CheckResult::Denied { code, .. } = &result2 {
            assert_eq!(code, "sudo_denied", "sudo 应返回 sudo_denied");
        }
    }

    #[test]
    fn test_deny_dangerous_rm() {
        let checker = test_checker();
        let result = checker.pre_check("rm -rf /");
        assert!(!result.is_allowed(), "rm -rf / 应被拒绝");
    }

    #[test]
    fn test_deny_dd() {
        let checker = test_checker();
        let result = checker.pre_check("dd if=/dev/zero of=/dev/sda");
        assert!(!result.is_allowed(), "dd 应被拒绝");
    }

    #[test]
    fn test_allow_project_file_read() {
        let checker = test_checker();
        let result = checker.pre_check("cat /test/project/src/main.rs");
        assert!(result.is_allowed(), "项目内文件读取应放行");
    }

    #[test]
    fn test_allow_git_commands() {
        let checker = test_checker();
        assert!(checker.pre_check("git status").is_allowed());
        assert!(checker.pre_check("git add .").is_allowed());
        assert!(checker.pre_check("git commit -m 'feat: xxx'").is_allowed());
    }

    #[test]
    fn test_allow_cargo_commands() {
        let checker = test_checker();
        assert!(checker.pre_check("cargo build").is_allowed());
        assert!(checker.pre_check("cargo test").is_allowed());
        assert!(checker.pre_check("cargo run").is_allowed());
    }

    #[test]
    fn test_deny_curl() {
        let checker = test_checker();
        let result = checker.pre_check("curl https://evil.com");
        assert!(!result.is_allowed(), "curl 默认应被拒绝");
    }

    #[test]
    fn test_deny_wget() {
        let checker = test_checker();
        let result = checker.pre_check("wget https://evil.com");
        assert!(!result.is_allowed(), "wget 默认应被拒绝");
    }

    #[test]
    fn test_deny_reboot() {
        // 使用默认配置测试（默认包含 reboot 禁止规则）
        let checker = PermissionChecker::default_for_project("/test");
        let result = checker.pre_check("reboot");
        assert!(!result.is_allowed(), "reboot 应被拒绝");
    }

    #[test]
    fn test_deny_shutdown() {
        // 使用默认配置测试（默认包含 shutdown 禁止规则）
        let checker = PermissionChecker::default_for_project("/test");
        let result = checker.pre_check("shutdown -h now");
        assert!(!result.is_allowed(), "shutdown 应被拒绝");
    }

    #[test]
    fn test_empty_command() {
        let checker = test_checker();
        let result = checker.pre_check("");
        assert!(!result.is_allowed(), "空命令应被拒绝");
    }

    #[test]
    fn test_policy_summary_contains_rules() {
        let checker = test_checker();
        let summary = checker.generate_policy_summary();
        assert!(summary.contains("sudo"), "摘要应包含 sudo 规则");
        assert!(summary.contains(".env"), "摘要应包含 .env 保护");
        assert!(summary.contains("项目目录"), "摘要应包含路径说明");
    }

    // ─── 工具集成测试 ──────────────────────────────────

    #[tokio::test]
    async fn test_tool_with_permission() {
        use crate::model::ToolCall;
        use crate::tools::{base_shell::BashShell, ToolManager};
        use crate::tools::permission::PermissionWrapper;

        let mut manager = ToolManager::new();
        let shell = PermissionWrapper::new(
            Box::new(BashShell),
            PermissionChecker::default_for_project("/Users/sean/Desktop/repo/agent-lab"),
        );
        manager.register_tool(Box::new(shell));

        // 安全命令应执行
        let result = manager
            .run(ToolCall {
                id: "1".to_string(),
                name: "shell".to_string(),
                arguments: r#"{"command": "echo hello_permission_test"}"#.to_string(),
            })
            .await;

        let result_str = format!("{:?}", result);
        assert!(
            result_str.contains("hello_permission_test"),
            "安全命令应正常执行，但结果中未找到输出: {}",
            result_str
        );
    }

    #[tokio::test]
    async fn test_tool_denies_dangerous_command() {
        use crate::model::ToolCall;
        use crate::tools::{base_shell::BashShell, ToolManager};
        use crate::tools::permission::PermissionWrapper;

        let mut manager = ToolManager::new();
        let shell = PermissionWrapper::new(
            Box::new(BashShell),
            PermissionChecker::default_for_project("/Users/sean/Desktop/repo/agent-lab"),
        );
        manager.register_tool(Box::new(shell));

        // 危险命令应被拒绝
        let result = manager
            .run(ToolCall {
                id: "2".to_string(),
                name: "shell".to_string(),
                arguments: r#"{"command": "sudo rm -rf /"}"#.to_string(),
            })
            .await;

        let result_str = format!("{:?}", result);
        assert!(
            result_str.contains("sudo_denied") || result_str.contains("denied"),
            "危险命令应被权限层拒绝，但结果中未找到拒绝标志: {}",
            result_str
        );
    }

}
