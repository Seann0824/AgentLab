use std::pin::Pin;

use futures_util::Stream;

pub type ToolStream = Pin<Box<dyn Stream<Item = ToolEvent> + Send + 'static>>;

#[derive(Debug, Clone)]
pub enum ToolEvent {
    Progress(String),
    Done(serde_json::Value),
    Err(String),
}

/// 工具执行错误类型。
///
/// 将工具链中的错误从松散字符串提升为结构化类型，
/// 便于区分错误性质并统一转换为模型可理解的提示。
#[derive(Debug, Clone)]
pub enum ToolError {
    /// 调用方参数缺失或非法。
    InvalidArgument(String),
    /// 外部服务返回错误（如 SerpApi、Ollama、Neo4j、PG 等）。
    ExternalService { service: &'static str, message: String },
    /// 工具内部异常，包括捕获到的 panic。
    Internal(String),
    /// 目标资源不存在。
    NotFound(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::InvalidArgument(msg) => write!(f, "参数错误: {}", msg),
            ToolError::ExternalService { service, message } => {
                write!(f, "外部服务 {} 错误: {}", service, message)
            }
            ToolError::Internal(msg) => write!(f, "工具内部错误: {}", msg),
            ToolError::NotFound(msg) => write!(f, "未找到: {}", msg),
        }
    }
}

impl std::error::Error for ToolError {}

impl ToolError {
    /// 生成适合返回给 LLM 的简短错误描述。
    pub fn to_agent_message(&self) -> String {
        match self {
            ToolError::InvalidArgument(msg) => {
                format!("调用失败，参数有误：{}。请检查后重试。", msg)
            }
            ToolError::ExternalService { service, message } => {
                format!(
                    "调用 {} 服务失败：{}。这通常是外部服务临时问题，你可以尝试重试或换种方式提问。",
                    service, message
                )
            }
            ToolError::Internal(msg) => {
                format!(
                    "工具内部错误：{}。请稍后重试，或跳过该工具直接回答。",
                    msg
                )
            }
            ToolError::NotFound(msg) => {
                format!("未找到相关内容：{}。", msg)
            }
        }
    }

    /// 快速构造外部服务错误。
    pub fn external(service: &'static str, message: impl Into<String>) -> Self {
        ToolError::ExternalService {
            service,
            message: message.into(),
        }
    }
}

#[async_trait::async_trait]
pub trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters;
    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError>;
}