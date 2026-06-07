# 功能特性设计：权限沙箱层 (Permission Sandbox Layer)

> **优先级**: P0 - 最高优先级  
> **状态**: ✅ 已实现  
> **估算工时**: 2-3 天 (实际约 1 天完成)  

---

## 1. 背景与动机

### 1.1 重新思考工具架构

在分析架构时，我最初犯了一个错误——试图为文件操作（read_file、write_file、edit_file、list_dir）定义**独立的工具**。

但回顾项目本质：**Agent 的核心能力来自用户电脑上的终端（shell）**。所有文件操作、git 操作、编译、运行——这些都是 shell 命令可以完成的事情。我们不需要自定义每个工具，而是需要让 **`shell` 这一个工具变得安全可控**。

### 1.2 核心设计理念

```
不是 "为每个操作造一个工具"
而是 "给 shell 工具戴上安全缰绳"
```

当前 `BashShell` 工具的问题不是"功能太少"，而是**没有安全约束**——Agent 可以执行任意命令、读写任意路径、没有任何边界。

### 1.3 为什么这是最高优先级

| 理由 | 说明 |
|------|------|
| 🔐 **安全基线** | 没有权限层，Agent 可执行 `rm -rf /`、读取 `.env` 密钥、写入系统文件 |
| 🧩 **Skill 架构基础** | 未来 Agent 的所有"技能"都基于终端命令，权限层是所有技能的前置条件 |
| 🤖 **模型自主权** | 有了明确的权限规则，模型可以自主决策哪些操作可行，减少拒绝或犯错 |
| 📐 **架构正交性** | 权限层独立于工具实现，一次构建，所有工具（未来多个）都能复用 |

---

## 2. 功能需求

### 2.1 权限模型

```
┌─────────────────────────────────────────────┐
│              PermissionSandbox               │
│                                              │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  │
│  │ 路径规则  │  │ 命令规则  │  │ 资源规则   │  │
│  │ (Path)   │  │ (Cmd)    │  │ (Resource)│  │
│  └──────────┘  └──────────┘  └───────────┘  │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │       检查引擎 (Check Engine)        │    │
│  │  pre_check() → 执行前拦截            │    │
│  │  post_check() → 结果后过滤           │    │
│  └──────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
```

### 2.2 核心需求

| ID | 需求描述 | 验收标准 |
|----|---------|---------|
| P-01 | **路径白名单**：Agent 只能在项目目录内读写文件 | 尝试 `cat /etc/passwd` 被拒绝，提示路径不在白名单 |
| P-02 | **命令黑名单**：禁止执行高危命令 | `rm -rf /`、`sudo`、`dd`、`:(){ :\|:& };:` 被拦截 |
| P-03 | **敏感文件保护**：禁止读取/写入 `.env`、`~/.ssh/*`、`/etc/*` | 读取 `.env` 被拒绝，提示包含敏感信息 |
| P-04 | **.git 目录保护**：禁止直接修改 `.git/` 内部文件 | `rm -rf .git` 被拦截 |
| P-05 | **网络限制**：默认禁止 curl/wget 到外部地址（可选放行） | `curl http://evil.com` 被拦截 |
| P-06 | **拒绝原因反馈**：被拒绝时返回结构化错误，说明违反的规则 | 返回 `{ code, rule, message }` 让模型理解边界 |
| P-07 | **规则可配置**：权限规则通过配置文件声明 | 支持 YAML/TOML 配置，无需改代码调整规则 |

### 2.3 非功能性需求

| 需求 | 指标 |
|------|------|
| 性能 | 权限检查耗时 < 1ms（不显著增加命令执行延迟） |
| 可扩展 | 新增规则类型无需修改检查引擎核心代码 |
| 可观测 | 权限命中记录日志，便于审计 |

---

## 3. 技术方案

### 3.1 架构位置

```
src/
├── main.rs
├── tools/
│   ├── mod.rs                    # ToolManager（已有）
│   ├── types.rs                  # Tool trait（已有）
│   ├── base_shell/
│   │   └── mod.rs                # BashShell（已有——需要集成权限检查）
│   └── permission/               # 新增：权限沙箱层
│       ├── mod.rs                # 模块导出 & PermissionSandbox 核心
│       ├── rules.rs              # 规则定义（路径规则、命令规则、资源规则）
│       ├── checker.rs            # 检查引擎（pre_check, post_check）
│       ├── config.rs             # 配置加载（从文件读取规则）
│       └── config.toml           # 默认权限配置
```

### 3.2 核心数据结构

#### 3.2.1 规则定义 (`rules.rs`)

```rust
/// 规则动作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleAction {
    Allow,   // 放行
    Deny,    // 拒绝
}

/// 路径规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRule {
    pub action: RuleAction,
    pub pattern: String,           // glob 模式，如 "/Users/sean/Desktop/repo/agent-lab/**"
    pub description: String,       // 规则说明，返回给模型
}

/// 命令规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRule {
    pub action: RuleAction,
    pub pattern: String,           // glob 匹配命令名，如 "rm", "sudo", "curl"
    pub args_constraint: Option<String>, // 参数约束，如 "rm -rf /" 匹配
    pub description: String,
}

/// 敏感文件规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitiveFileRule {
    pub action: RuleAction,
    pub pattern: String,           // glob 匹配路径
    pub read_allowed: bool,        // 是否允许读取
    pub write_allowed: bool,       // 是否允许写入
    pub description: String,
}

/// 完整权限配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionConfig {
    pub version: String,
    pub project_root: String,
    pub path_rules: Vec<PathRule>,
    pub command_rules: Vec<CommandRule>,
    pub sensitive_file_rules: Vec<SensitiveFileRule>,
}
```

#### 3.2.2 检查结果

```rust
/// 权限检查结果
#[derive(Debug, Clone)]
pub enum CheckResult {
    /// 通过
    Allowed {
        rule: String,       // 匹配的规则名
    },
    /// 被拒绝
    Denied {
        code: String,       // 错误码
        rule: String,       // 触发的规则
        message: String,    // 人类可读的拒绝原因
        suggestion: String, // 建议的替代操作
    },
}

impl CheckResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, CheckResult::Allowed { .. })
    }
    
    /// 转成返回给模型的 JSON 错误
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
            _ => unreachable!(),
        }
    }
}
```

### 3.3 检查引擎 (`checker.rs`)

#### 3.3.1 命令解析

在执行 shell 命令前，先做词法解析，提取出命令名、参数、操作路径：

```rust
/// 解析 shell 命令，提取结构化信息用于权限检查
pub struct ParsedCommand {
    /// 原始命令
    pub raw: String,
    /// 主命令（如 "cat", "rm", "curl"）
    pub command: String,
    /// 参数列表
    pub args: Vec<String>,
    /// 涉及的文件路径（从参数中提取）
    pub file_paths: Vec<String>,
    /// 涉及的 URL（从参数中提取）
    pub urls: Vec<String>,
    /// 是否包含 sudo
    pub has_sudo: bool,
    /// 是否包含管道/重定向
    pub has_pipe_or_redirect: bool,
}

impl ParsedCommand {
    /// 从原始命令解析
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let parts = shell_words::split(trimmed).unwrap_or_default();
        
        let has_sudo = parts.first().map(|s| s == "sudo").unwrap_or(false);
        let command = if has_sudo {
            parts.get(1).cloned().unwrap_or_default()
        } else {
            parts.first().cloned().unwrap_or_default()
        };
        
        let args: Vec<String> = parts.iter().skip(if has_sudo { 2 } else { 1 }).cloned().collect();
        
        // 提取文件路径（不以 - 开头的参数，且看起来像路径）
        let file_paths: Vec<String> = args
            .iter()
            .filter(|a| !a.starts_with('-') && (a.contains('/') || std::path::Path::new(a).exists()))
            .cloned()
            .collect();
        
        // 提取 URL
        let urls: Vec<String> = args
            .iter()
            .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
            .cloned()
            .collect();
        
        let has_pipe_or_redirect = trimmed.contains('|') || trimmed.contains('>') || trimmed.contains('<');
        
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
}
```

> **注意**：这需要一个依赖 `shell_words`（或手写简单解析器），用于正确分割 shell 命令。如果不想引入新依赖，也可以用简单的空格分割 + 引号处理。

#### 3.3.2 检查引擎

```rust
/// 权限检查引擎
pub struct PermissionChecker {
    config: PermissionConfig,
}

impl PermissionChecker {
    pub fn new(config: PermissionConfig) -> Self {
        Self { config }
    }
    
    /// 前置检查：命令执行前
    pub fn pre_check(&self, raw_command: &str) -> CheckResult {
        let parsed = ParsedCommand::parse(raw_command);
        
        // 1. 先检查命令规则
        for rule in &self.config.command_rules {
            if self.matches_command(&parsed, rule) {
                match rule.action {
                    RuleAction::Deny => {
                        return CheckResult::Denied {
                            code: "command_denied".to_string(),
                            rule: rule.description.clone(),
                            message: format!("命令 '{}' 被安全规则禁止: {}", parsed.command, rule.description),
                            suggestion: self.suggest_alternative(&parsed.command),
                        };
                    }
                    RuleAction::Allow => {
                        // 明确放行的命令继续后续检查
                        break;
                    }
                }
            }
        }
        
        // 2. 检查 sudo
        if parsed.has_sudo {
            return CheckResult::Denied {
                code: "sudo_denied".to_string(),
                rule: "禁止使用 sudo".to_string(),
                message: "Agent 不允许使用 sudo 执行命令，这会影响系统安全。".to_string(),
                suggestion: "尝试不使用 sudo 的命令".to_string(),
            };
        }
        
        // 3. 检查涉及的文件路径
        for file_path in &parsed.file_paths {
            let abs_path = self.resolve_path(file_path);
            for rule in &self.config.sensitive_file_rules {
                if self.matches_glob(&abs_path, &rule.pattern) {
                    let is_write = self.is_write_operation(&parsed.command);
                    if (is_write && !rule.write_allowed) || (!is_write && !rule.read_allowed) {
                        return CheckResult::Denied {
                            code: "sensitive_file_denied".to_string(),
                            rule: rule.description.clone(),
                            message: format!("访问被禁止: {} 是敏感文件", file_path),
                            suggestion: "此文件受保护，不允许访问".to_string(),
                        };
                    }
                }
            }
            
            // 检查路径白名单
            let in_allowed = self.config.path_rules.iter().any(|r| {
                r.action == RuleAction::Allow && self.matches_glob(&abs_path, &r.pattern)
            });
            if !in_allowed {
                return CheckResult::Denied {
                    code: "path_not_allowed".to_string(),
                    rule: "路径白名单".to_string(),
                    message: format!("路径不在允许的操作范围内: {}", file_path),
                    suggestion: format!("请在项目目录 {} 内操作", self.config.project_root),
                };
            }
        }
        
        // 4. 检查网络请求
        for url in &parsed.urls {
            if !self.is_url_allowed(url) {
                return CheckResult::Denied {
                    code: "network_denied".to_string(),
                    rule: "网络访问限制".to_string(),
                    message: format!("不允许网络请求到: {}", url),
                    suggestion: "如需网络请求，请联系管理员配置放行规则".to_string(),
                };
            }
        }
        
        CheckResult::Allowed {
            rule: "所有检查通过".to_string()
        }
    }
    
    /// 是否是写操作
    fn is_write_operation(&self, command: &str) -> bool {
        matches!(
            command,
            "rm" | "mv" | "cp" | "dd" | "chmod" | "chown" 
            | "touch" | "mkdir" | "ln" | "truncate"
            | ">"
        )
    }
    
    /// 解析路径为绝对路径
    fn resolve_path(&self, path: &str) -> String {
        let base = std::path::Path::new(&self.config.project_root);
        let target = if path.starts_with('/') {
            std::path::Path::new(path).to_path_buf()
        } else {
            base.join(path)
        };
        // 简化处理，实际可能需要 canonicalize
        target.to_string_lossy().to_string()
    }
    
    /// Glob 匹配
    fn matches_glob(&self, path: &str, pattern: &str) -> bool {
        glob_match::glob_match(pattern, path)
    }
    
    /// 命令匹配
    fn matches_command(&self, parsed: &ParsedCommand, rule: &CommandRule) -> bool {
        glob_match::glob_match(&rule.pattern, &parsed.command)
    }
    
    /// URL 是否允许
    fn is_url_allowed(&self, url: &str) -> bool {
        // 默认只允许 API 调用，外部 URL 需要显式配置
        false
    }
    
    /// 建议替代命令
    fn suggest_alternative(&self, command: &str) -> String {
        match command {
            "sudo" => "使用普通权限执行".to_string(),
            "rm" => "考虑使用 trash 或先确认文件路径是否正确".to_string(),
            "dd" => "禁止使用 dd 命令".to_string(),
            "curl" | "wget" => "网络请求默认禁止，如需使用请配置放行规则".to_string(),
            _ => format!("尝试寻找替代方案，或确认是否是必要的操作".to_string()),
        }
    }
}
```

### 3.4 默认权限配置 (`config.toml`)

```toml
# agent-lab 权限沙箱配置
version = "1.0"
project_root = "/Users/sean/Desktop/repo/agent-lab"

# ─── 路径白名单 ──────────────────────────────
# Agent 只能在这些路径下操作文件
[[path_rules]]
action = "Allow"
pattern = "/Users/sean/Desktop/repo/agent-lab/**"
description = "项目目录"

# ─── 命令黑名单 ──────────────────────────────
# 这些命令禁止执行
[[command_rules]]
action = "Deny"
pattern = "sudo"
description = "禁止使用 sudo 提升权限"

[[command_rules]]
action = "Deny"
pattern = "dd"
description = "禁止使用 dd 直接读写设备"

[[command_rules]]
action = "Deny"
pattern = "passwd"
description = "禁止修改系统密码"

[[command_rules]]
action = "Deny"
pattern = "chown"
description = "禁止修改文件所有者"

[[command_rules]]
action = "Deny"
pattern = "chmod"
args_constraint = "777"
description = "禁止设置 777 权限"

[[command_rules]]
action = "Deny"
pattern = "rm"
args_constraint = "-rf /"
description = "禁止递归删除根目录"

[[command_rules]]
action = "Deny"
pattern = "rm"
args_constraint = "-rf ~"
description = "禁止递归删除家目录"

[[command_rules]]
action = "Deny"
pattern = "kill"
description = "禁止杀死进程（可选放行）"

[[command_rules]]
action = "Deny"
pattern = "reboot"
description = "禁止重启系统"

[[command_rules]]
action = "Deny"
pattern = "shutdown"
description = "禁止关机"

# ─── 敏感文件保护 ─────────────────────────────
# 这些文件禁止读写
[[sensitive_file_rules]]
action = "Deny"
pattern = "**/.env"
read_allowed = false
write_allowed = false
description = "环境变量文件（含 API Key）"

[[sensitive_file_rules]]
action = "Deny"
pattern = "**/.git/**"
read_allowed = false    # git 对象读取可能允许，写入绝对禁止
write_allowed = false
description = "Git 内部数据"

[[sensitive_file_rules]]
action = "Deny"
pattern = "~/.ssh/**"
read_allowed = false
write_allowed = false
description = "SSH 密钥文件"

[[sensitive_file_rules]]
action = "Deny"
pattern = "**/id_rsa*"
read_allowed = false
write_allowed = false
description = "RSA 私钥文件"

[[sensitive_file_rules]]
action = "Deny"
pattern = "**/*.pem"
read_allowed = false
write_allowed = false
description = "PEM 密钥文件"

# ─── 网络规则 ────────────────────────────────
# 默认禁止所有外部网络请求
# 如需启用 API 调用，可放行特定域名
# [[network_rules]]
# action = "Allow"
# pattern = "https://api.deepseek.com"
# description = "DeepSeek API"
```

### 3.5 与现有 BashShell 的集成

修改 `base_shell/mod.rs`，在执行命令前增加权限检查：

```rust
// 在 BashShell::execute() 中增加权限检查

fn execute(&self, args: serde_json::Value) -> ToolStream {
    let command = args["command"].as_str().unwrap_or("").to_string();
    let (tx, rx) = mpsc::channel(1);

    tokio::spawn(async move {
        // 1. 前置权限检查
        let checker = PermissionChecker::from_default_config();
        let check_result = checker.pre_check(&command);
        
        if !check_result.is_allowed() {
            let _ = tx.send(ToolEvent::Done(
                check_result.to_error_json()
            )).await;
            return;
        }
        
        // 2. 原有的命令执行逻辑...
        // (保持不变)
    });
    
    Box::pin(ReceiverStream::new(rx))
}
```

但更好的方式是**把权限检查作为一个中间件/装饰器**，不侵入 BashShell 本身：

```rust
/// 权限装饰器：包装任意 Tool，在执行前加权限检查
pub struct PermissionWrapper {
    inner: Box<dyn Tool>,
    checker: PermissionChecker,
}

impl Tool for PermissionWrapper {
    fn name(&self) -> &str { self.inner.name() }
    fn description(&self) -> &str { self.inner.description() }
    fn parameters_schema(&self) -> serde_json::Value { self.inner.parameters_schema() }
    
    fn execute(&self, args: serde_json::Value) -> ToolStream {
        // 提取命令参数（需要各工具配合暴露参数）
        let command = args["command"].as_str().unwrap_or("");
        let check = self.checker.pre_check(command);
        
        if !check.is_allowed() {
            // 拒绝执行
            let (tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                let _ = tx.send(ToolEvent::Done(check.to_error_json())).await;
            });
            return Box::pin(ReceiverStream::new(rx));
        }
        
        // 通过，执行原始工具
        self.inner.execute(args)
    }
}
```

这样注册方式变为：

```rust
fn initial_tool_manager() -> ToolManager {
    let mut tool_manager = ToolManager::new();
    let shell = PermissionWrapper::new(
        Box::new(BashShell),
        PermissionChecker::from_default_config(),
    );
    tool_manager.register_tool(Box::new(shell));
    tool_manager
}
```

### 3.6 规则热加载与模型提示

为了让 **AI 模型知道边界在哪**（避免反复尝试被拒的操作），权限层应该能在系统提示词中动态注入摘要：

```rust
impl PermissionChecker {
    /// 生成权限规则的文本摘要，注入到系统提示词中
    pub fn generate_policy_summary(&self) -> String {
        let mut summary = String::from("## ⚠️ 安全权限规则\n\n");
        
        summary.push_str("### 允许的操作路径\n");
        for rule in &self.config.path_rules {
            if matches!(rule.action, RuleAction::Allow) {
                summary.push_str(&format!("- ✅ 路径: `{}` — {}\n", rule.pattern, rule.description));
            }
        }
        
        summary.push_str("\n### 禁止的命令\n");
        for rule in &self.config.command_rules {
            if matches!(rule.action, RuleAction::Deny) {
                summary.push_str(&format!("- 🚫 `{}` — {}\n", rule.pattern, rule.description));
            }
        }
        
        summary.push_str("\n### 受保护的文件\n");
        for rule in &self.config.sensitive_file_rules {
            if matches!(rule.action, RuleAction::Deny) {
                summary.push_str(&format!("- 🔒 `{}` — {}\n", rule.pattern, rule.description));
            }
        }
        
        summary.push_str("\n> 如果某个操作被拒绝，请阅读返回的错误信息中的 `suggestion` 字段寻求替代方案。\n");
        summary
    }
}
```

然后在 `main.rs` 中，将规则摘要注入系统提示词：

```rust
fn main() {
    let policy_summary = checker.generate_policy_summary();
    let messages = vec![
        ChatMessage::system(format!(
            "你当前工作的目录为 ...\n\n{}",
            policy_summary
        )),
    ];
}
```

---

## 4. 依赖分析

| 依赖 | 用途 | 是否新增 |
|------|------|---------|
| `glob-match` 或 `globset` | Glob 模式匹配路径和命令 | ✅ 新增 |
| `shell-words` | 解析 shell 命令参数（处理引号转义） | ✅ 可选 |
| `toml` / `serde` | 读取 TOML 配置文件 | ✅ 新增（或复用 serde） |
| 已有依赖 | `anyhow`, `serde_json`, `tokio` | 已有 |

如果不想引入太多依赖，可以手写简单的：
- **Glob 匹配**：简单的 `*`、`**` 通配符匹配（几十行代码）
- **Shell 解析**：处理引号的简单分割器（实现也简单）

---

## 5. 默认权限策略总结

| 类别 | 默认行为 | 说明 |
|------|---------|------|
| **项目目录** | ✅ **允许** | `/Users/sean/Desktop/repo/agent-lab/**` |
| **系统目录** | ❌ **拒绝** | `/etc/`、`/usr/`、`/tmp/` 等 |
| **sudo** | ❌ **拒绝** | 不可提权 |
| **敏感文件** | ❌ **拒绝** | `.env`、`.git/`、`~/.ssh/`、`*.pem` |
| **高危命令** | ❌ **拒绝** | `dd`, `reboot`, `shutdown`, `kill`, `passwd` |
| **rm -rf /** | ❌ **拒绝** | 参数约束匹配 |
| **网络请求** | ❌ **拒绝** | curl/wget 默认禁止（可配置放行） |
| **Git 操作** | ✅ **允许** | `git add/commit/push` 等正常使用 |
| **Cargo 操作** | ✅ **允许** | `cargo build/test/run` 正常使用 |
| **文件读写** | ✅ **允许** | 项目目录内 `cat/vim/echo/重定向` 正常 |

---

## 6. 分层实现计划

```
✅ Phase 1: 规则引擎（0.5天）
├── ✅ 定义规则数据结构 (PermissionConfig, PathRule, CommandRule)
├── ✅ 实现 Glob 匹配器
├── ✅ 实现 Shell 命令解析器 (简易版)
└── ✅ 单元测试: 规则解析、命令解析

✅ Phase 2: 检查引擎（0.5天）
├── ✅ 实现 PermissionChecker::pre_check()
├── ✅ 实现命令黑名单匹配
├── ✅ 实现路径白名单匹配
├── ✅ 实现敏感文件保护
└── ✅ 单元测试: 各种场景的权限检查

✅ Phase 3: 集成（0.5天）
├── ✅ 实现 PermissionWrapper 装饰器
├── ✅ 写入默认 config.toml
├── ✅ 集成到 BashShell + main.rs
├── ✅ 权限规则注入系统提示词
└── ✅ 集成测试: 完整流程

✅ Phase 4: 配置系统（0.5天）
├── ✅ 配置文件热加载
├── ✅ 配置校验
├── ✅ 规则摘要生成
└── ✅ 文档

总计: 2天（实际已全部完成 ✅）
```

---

## 7. 测试策略

### 7.1 权限检查测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    fn test_checker() -> PermissionChecker {
        let config = PermissionConfig {
            version: "test".into(),
            project_root: "/test/project".into(),
            path_rules: vec![
                PathRule { action: Allow, pattern: "/test/project/**".into(), .. }
            ],
            command_rules: vec![
                CommandRule { action: Deny, pattern: "sudo".into(), .. },
                CommandRule { action: Deny, pattern: "rm".into(), args_constraint: Some("-rf /".into()), .. },
            ],
            sensitive_file_rules: vec![
                SensitiveFileRule { action: Deny, pattern: "**/.env".into(), read_allowed: false, write_allowed: false, .. },
            ],
        };
        PermissionChecker::new(config)
    }
    
    #[test]
    fn test_allow_safe_command() {
        let checker = test_checker();
        let result = checker.pre_check("ls -la");
        assert!(result.is_allowed());
    }
    
    #[test]
    fn test_deny_sudo() {
        let checker = test_checker();
        let result = checker.pre_check("sudo rm -rf /");
        assert!(!result.is_allowed());
        assert_eq!(result.code(), "sudo_denied");
    }
    
    #[test]
    fn test_deny_dangerous_rm() {
        let checker = test_checker();
        let result = checker.pre_check("rm -rf /");
        assert!(!result.is_allowed());
    }
    
    #[test]
    fn test_deny_sensitive_file() {
        let checker = test_checker();
        let result = checker.pre_check("cat /test/project/.env");
        assert!(!result.is_allowed());
        assert_eq!(result.code(), "sensitive_file_denied");
    }
    
    #[test]
    fn test_allow_project_file() {
        let checker = test_checker();
        let result = checker.pre_check("cat /test/project/src/main.rs");
        assert!(result.is_allowed());
    }
    
    #[test]
    fn test_deny_path_escape() {
        let checker = test_checker();
        let result = checker.pre_check("cat /etc/passwd");
        assert!(!result.is_allowed());
        assert_eq!(result.code(), "path_not_allowed");
    }
    
    #[test]
    fn test_allow_git_commands() {
        let checker = test_checker();
        assert!(checker.pre_check("git status").is_allowed());
        assert!(checker.pre_check("git add .").is_allowed());
        assert!(checker.pre_check("git commit -m 'feat: xxx'").is_allowed());
    }
    
    #[test]
    fn test_deny_dd() {
        let checker = test_checker();
        let result = checker.pre_check("dd if=/dev/zero of=/dev/sda");
        assert!(!result.is_allowed());
    }
}
```

### 7.2 集成测试

```rust
#[tokio::test]
async fn test_tool_with_permission() {
    let mut manager = ToolManager::new();
    let shell = PermissionWrapper::new(
        Box::new(BashShell),
        PermissionChecker::from_default_config(),
    );
    manager.register_tool(Box::new(shell));
    
    // 安全的命令应该执行
    let result = manager.run(ToolCall {
        id: "1".into(),
        name: "shell".into(),
        arguments: r#"{"command": "echo hello"}"#.into(),
    }).await;
    assert!(result.to_string().contains("hello"));
    
    // 危险命令应该被拒绝
    let result = manager.run(ToolCall {
        id: "2".into(),
        name: "shell".into(),
        arguments: r#"{"command": "sudo rm -rf /"}"#.into(),
    }).await;
    assert!(result.to_string().contains("denied"));
}
```

---

## 8. 后续扩展方向

| 阶段 | 功能 | 说明 |
|------|------|------|
| V1 | **权限沙箱** | 本方案：路径白名单 + 命令黑名单 + 敏感文件保护 |
| V2 | **Skill 注册系统** | 允许用户注册"技能"（封装好的命令模板），如 `git_commit(msg)`、`cargo_build()` |
| V3 | **精细权限** | 按技能粒度授权，用户可授权特定技能访问特定路径 |
| V4 | **审计日志** | 记录所有命令执行和权限命中，支持回放查看 Agent 行为 |
| V5 | **动态授权** | 命令被拒绝时，提示用户确认是否放行（类似移动端权限弹窗） |
| V6 | **多沙箱** | 不同项目不同沙箱配置，支持多项目切换 |

### V2 展望: Skill 注册系统

```rust
// 未来的 Skill 注册方式（基于权限沙箱之上）
pub struct Skill {
    pub name: String,
    pub description: String,
    pub command_template: String,  // 如 "git commit -m '{message}'"
    pub parameters: Vec<Parameter>,
    pub permission: PermissionConfig, // 该技能的权限
}

// 注册一个 Git Commit 技能
skill_manager.register(Skill {
    name: "git_commit",
    description: "提交 Git 变更",
    command_template: "git add -A && git commit -m '{message}'",
    parameters: vec![
        Parameter { name: "message", type: "string", required: true }
    ],
    permission: PermissionConfig {
        path_rules: vec![Allow("**".into())],
        command_rules: vec![Allow("git".into())],
        ..Default::default()
    }
});
```

---

## 9. 风险评估

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|---------|
| 命令解析不完整导致绕过 | 高 | 中 | 使用成熟的 `shell-words` 库解析；黑名单+白名单双重校验 |
| 路径穿越绕过 `starts_with` | 高 | 低 | 使用 `canonicalize` 解析真实路径后再比较；禁止符号链接逃逸 |
| 模型频繁尝试被拒操作 | 低 | 高 | 在系统提示词中注入规则摘要，让模型"知道边界" |
| Glob 匹配性能问题 | 低 | 低 | 规则数量通常 < 50 条，单次检查 < 1ms |
| 配置错误导致安全漏洞 | 高 | 低 | 配置校验 + 默认安全的配置模板 |

---

> **设计原则**: 权限沙箱应该做到"默认安全、按需放行"。  
> **核心哲学**: 不是限制 Agent 的能力，而是让能力在安全边界内发挥。  
> 和用户终端技能（Skill）的理念一脉相承——终端命令就是技能，权限层是技能的缰绳。

