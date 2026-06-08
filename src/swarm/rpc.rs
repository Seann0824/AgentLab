// src/swarm/rpc.rs
// JSON-RPC 2.0 协议 — Agent 间通信的消息格式
//
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

use serde::{Deserialize, Serialize};
use std::fmt;

// ===================================================================
// JSON-RPC 2.0 基本类型
// ===================================================================

/// JSON-RPC 2.0 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,           // "2.0"
    pub id: String,                // 请求 ID（UUID）
    pub method: String,            // 方法名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: uuid_v4(),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,           // "2.0"
    pub id: String,                // 对应请求 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// 创建成功响应
    pub fn success(id: impl Into<String>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    /// 创建错误响应
    pub fn error(id: impl Into<String>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// 是否成功
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// JSON-RPC 2.0 错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

// ===================================================================
// 预定义的 RPC 方法
// ===================================================================

/// 蜂群 RPC 方法枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwarmMethod {
    // Agent 注册与发现
    Register,
    Unregister,
    Heartbeat,
    QuerySwarm,

    // 任务派发
    DispatchTask,
    CancelTask,
    TaskResult,

    // 事件
    Event,
    Broadcast,
}

impl SwarmMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            SwarmMethod::Register => "register",
            SwarmMethod::Unregister => "unregister",
            SwarmMethod::Heartbeat => "heartbeat",
            SwarmMethod::QuerySwarm => "query_swarm",
            SwarmMethod::DispatchTask => "dispatch_task",
            SwarmMethod::CancelTask => "cancel_task",
            SwarmMethod::TaskResult => "task_result",
            SwarmMethod::Event => "event",
            SwarmMethod::Broadcast => "broadcast",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "register" => Some(SwarmMethod::Register),
            "unregister" => Some(SwarmMethod::Unregister),
            "heartbeat" => Some(SwarmMethod::Heartbeat),
            "query_swarm" => Some(SwarmMethod::QuerySwarm),
            "dispatch_task" => Some(SwarmMethod::DispatchTask),
            "cancel_task" => Some(SwarmMethod::CancelTask),
            "task_result" => Some(SwarmMethod::TaskResult),
            "event" => Some(SwarmMethod::Event),
            "broadcast" => Some(SwarmMethod::Broadcast),
            _ => None,
        }
    }
}

// ===================================================================
// 工具函数
// ===================================================================

/// 生成简单的 UUID v4（不依赖外部库）
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.as_nanos();
    let random_part: u64 = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        nanos.hash(&mut hasher);
        hasher.finish()
    };
    format!("{:016x}{:016x}", nanos, random_part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_serialization() {
        let req = JsonRpcRequest::new("register", Some(serde_json::json!({
            "agent_id": "test-agent",
            "agent_type": "memory"
        })));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"register\""));
        assert!(json.contains("\"agent_id\":\"test-agent\""));
    }

    #[test]
    fn test_json_rpc_response() {
        let resp = JsonRpcResponse::success("req-1", serde_json::json!({"status": "ok"}));
        assert!(resp.is_success());
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
    }

    #[test]
    fn test_json_rpc_error_response() {
        let resp = JsonRpcResponse::error("req-2", -32601, "Method not found");
        assert!(!resp.is_success());
        assert_eq!(resp.error.as_ref().unwrap().code, -32601);
    }

    #[test]
    fn test_swarm_method_roundtrip() {
        for method in &[
            SwarmMethod::Register,
            SwarmMethod::Unregister,
            SwarmMethod::Heartbeat,
            SwarmMethod::QuerySwarm,
            SwarmMethod::DispatchTask,
            SwarmMethod::CancelTask,
            SwarmMethod::TaskResult,
            SwarmMethod::Event,
            SwarmMethod::Broadcast,
        ] {
            let s = method.as_str();
            let parsed = SwarmMethod::from_str(s).unwrap();
            assert_eq!(*method, parsed);
        }
    }
}
