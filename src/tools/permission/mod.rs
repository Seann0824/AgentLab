//! # 权限沙箱层 (Permission Sandbox Layer)
//!
//! 本模块实现了 Agent 执行命令时的安全权限控制。
//!
//! ## 设计理念
//!
//! 不是为每个操作自定义工具，而是给 shell 工具戴上安全缰绳。
//! Agent 的核心能力来自终端命令，权限层是命令的安全边界。
//!
//! ## 架构
//!
//! ```text
//! PermissionWrapper (装饰器)
//!     │
//!     ├── ParsedCommand (命令解析)
//!     ├── PermissionChecker (检查引擎)
//!     │   ├── 命令黑名单检查
//!     │   ├── Sudo 检查
//!     │   ├── 敏感文件检查
//!     │   ├── 路径白名单检查
//!     │   └── 网络请求检查
//!     └── CheckResult (检查结果)
//! ```
//!
//! ## 使用方式
//!
//! ```rust,no_run
//! use agent_lab::tools::permission::{PermissionWrapper, PermissionChecker};
//! use agent_lab::tools::base_shell::BashShell;
//!
//! let shell = PermissionWrapper::new(
//!     Box::new(BashShell),
//!     PermissionChecker::default_for_project("/my/project"),
//! );
//! ```

pub mod checker;
pub mod config;
pub mod rules;

pub use checker::{CheckResult, PermissionChecker};
pub use config::PermissionConfigLoader;
pub use rules::PermissionConfig;

use std::pin::Pin;

use crate::tools::types::{Tool, ToolEvent, ToolStream};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// 权限装饰器：包装任意 Tool，在执行前增加权限检查
///
/// 使用 Decorator 模式，不侵入原始工具的实现代码。
/// 所有权限检查在 `execute()` 调用时触发，检查不通过则直接返回错误。
///
/// # 示例
///
/// ```rust,no_run
/// use agent_lab::tools::permission::{PermissionWrapper, PermissionChecker};
/// use agent_lab::tools::base_shell::BashShell;
///
/// let wrapped = PermissionWrapper::new(
///     Box::new(BashShell),
///     PermissionChecker::default_for_project("/Users/sean/Desktop/repo/agent-lab"),
/// );
/// ```
pub struct PermissionWrapper {
    inner: Box<dyn Tool>,
    checker: PermissionChecker,
}

impl PermissionWrapper {
    /// 创建一个新的权限包装器
    ///
    /// # 参数
    /// - `inner`：被包装的原始工具
    /// - `checker`：权限检查引擎
    pub fn new(inner: Box<dyn Tool>, checker: PermissionChecker) -> Self {
        Self { inner, checker }
    }

    /// 获取权限检查器的引用
    pub fn checker(&self) -> &PermissionChecker {
        &self.checker
    }

    /// 获取权限检查器的可变引用
    pub fn checker_mut(&mut self) -> &mut PermissionChecker {
        &mut self.checker
    }
}

impl Tool for PermissionWrapper {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.inner.parameters_schema()
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        // 提取命令参数（各工具需配合暴露 command 参数）
        let command = args["command"].as_str().unwrap_or("");

        // 执行前置权限检查
        let check = self.checker.pre_check(command);

        if !check.is_allowed() {
            // 权限检查未通过，返回结构化错误
            let (tx, rx) = mpsc::channel(1);
            let error_json = check.to_error_json();
            tokio::spawn(async move {
                let _ = tx.send(ToolEvent::Done(error_json)).await;
            });
            return Box::pin(ReceiverStream::new(rx));
        }

        // 权限检查通过，执行原始工具
        self.inner.execute(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::base_shell::BashShell;
    use crate::model::ToolCall;
    use crate::tools::ToolManager;

    #[test]
    fn test_wrapper_preserves_tool_identity() {
        let wrapped = PermissionWrapper::new(
            Box::new(BashShell),
            PermissionChecker::default_for_project("/test"),
        );
        assert_eq!(wrapped.name(), "shell");
        assert!(wrapped.description().contains("CLI"));
    }

    #[tokio::test]
    async fn test_wrapper_allows_safe_command() {
        let mut manager = ToolManager::new();
        let wrapped = PermissionWrapper::new(
            Box::new(BashShell),
            PermissionChecker::default_for_project("/Users/sean/Desktop/repo/agent-lab"),
        );
        manager.register_tool(Box::new(wrapped));

        let result = manager
            .run(ToolCall {
                id: "1".to_string(),
                name: "shell".to_string(),
                arguments: r#"{"command": "echo ok"}"#.to_string(),
            })
            .await;

        let result_str = format!("{:?}", result);
        assert!(result_str.contains("ok"), "安全命令应执行: {}", result_str);
    }

    #[tokio::test]
    async fn test_wrapper_blocks_dangerous_command() {
        let mut manager = ToolManager::new();
        let wrapped = PermissionWrapper::new(
            Box::new(BashShell),
            PermissionChecker::default_for_project("/Users/sean/Desktop/repo/agent-lab"),
        );
        manager.register_tool(Box::new(wrapped));

        let result = manager
            .run(ToolCall {
                id: "2".to_string(),
                name: "shell".to_string(),
                arguments: r#"{"command": "sudo rm -rf /"}"#.to_string(),
            })
            .await;

        let result_str = format!("{:?}", result);
        assert!(
            result_str.contains("sudo_denied"),
            "sudo 命令应被拒绝: {}",
            result_str
        );
    }
}
