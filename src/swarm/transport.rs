// src/swarm/transport.rs
// UDS 传输层 — 使用 Unix Domain Socket 进行 Agent 间通信
//
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};

use super::rpc::{JsonRpcRequest, JsonRpcResponse};

// ===================================================================
// 默认 Socket 路径
// ===================================================================

/// 默认的蜂群 Socket 目录
pub const DEFAULT_SWARM_SOCKET_DIR: &str = "/tmp/agent-lab";

/// 默认的蜂群 Socket 文件名
pub const DEFAULT_SWARM_SOCKET_FILE: &str = "swarm.sock";

/// 获取默认的 Socket 路径
pub fn default_socket_path() -> PathBuf {
    PathBuf::from(DEFAULT_SWARM_SOCKET_DIR).join(DEFAULT_SWARM_SOCKET_FILE)
}

// ===================================================================
// 连接信息
// ===================================================================

/// UDS 连接信息
#[derive(Debug, Clone)]
pub struct UdsConnection {
    pub agent_id: String,
    pub connected_at: SystemTime,
}

// ===================================================================
// UDS 服务器（Orchestrator 使用）
// ===================================================================

/// UDS 服务器 — Orchestrator 使用，监听 Agent 连接
pub struct UdsServer {
    listener: UnixListener,
    /// socket 路径（便于退出时清理）
    socket_path: PathBuf,
    /// 已建立的连接
    connections: HashMap<String, UdsConnection>,
}

impl UdsServer {
    /// 在指定路径创建 UDS 服务器
    pub async fn bind(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // 确保父目录存在
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context(format!("Failed to create socket dir: {:?}", parent))?;
        }

        // 删除已存在的 socket 文件
        if path.exists() {
            tokio::fs::remove_file(&path).await.ok();
        }

        let listener = UnixListener::bind(&path)
            .context(format!("Failed to bind UDS server at {:?}", path))?;

        eprintln!("🐝 [Swarm] UDS Server listening at {:?}", path);

        Ok(Self {
            listener,
            socket_path: path,
            connections: HashMap::new(),
        })
    }

    /// 接受一个新的 Agent 连接
    pub async fn accept(&mut self) -> Result<(String, UdsStream)> {
        let (stream, _addr) = self.listener.accept().await?;
        let (reader, writer) = stream.into_split();
        let mut stream = UdsStream { reader, writer };

        // 等待 Agent 发送注册消息
        let register_msg = stream.read_request().await?;
        let agent_id = register_msg
            .params
            .as_ref()
            .and_then(|p| p.get("agent_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("unknown-{}", self.connections.len()));

        let conn = UdsConnection {
            agent_id: agent_id.clone(),
            connected_at: SystemTime::now(),
        };
        self.connections.insert(agent_id.clone(), conn);

        eprintln!("🐝 [Swarm] Agent '{}' connected", agent_id);

        Ok((agent_id, stream))
    }

    /// 获取已连接的 Agent 数量
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// 获取所有已连接的 Agent ID
    pub fn connected_agents(&self) -> Vec<String> {
        self.connections.keys().cloned().collect()
    }

    /// 清理 socket 文件
    pub async fn cleanup(&self) {
        tokio::fs::remove_file(&self.socket_path).await.ok();
    }
}

impl Drop for UdsServer {
    fn drop(&mut self) {
        let path = self.socket_path.clone();
        let _ = std::fs::remove_file(&path);
    }
}

// ===================================================================
// UDS 客户端（Agent 使用）
// ===================================================================

/// UDS 客户端 — Agent 使用，连接到 Orchestrator
pub struct UdsClient {
    writer: OwnedWriteHalf,
    agent_id: String,
    /// 读取缓冲的行
    buf_reader: BufReader<OwnedReadHalf>,
}

impl UdsClient {
    /// 连接到指定的 UDS 服务器
    pub async fn connect(path: impl AsRef<Path>, agent_id: impl Into<String>) -> Result<Self> {
        let stream = UnixStream::connect(path.as_ref())
            .await
            .context(format!("Failed to connect to UDS at {:?}", path.as_ref()))?;

        let (reader, writer) = stream.into_split();
        let agent_id = agent_id.into();

        let mut client = Self {
            buf_reader: BufReader::new(reader),
            writer,
            agent_id,
        };

        // 发送注册消息
        let register_req = JsonRpcRequest::new(
            "register",
            Some(serde_json::json!({
                "agent_id": client.agent_id,
            })),
        );
        client.send_request(&register_req).await?;

        eprintln!(
            "🐝 [Swarm] Client '{}' connected to server",
            client.agent_id
        );

        Ok(client)
    }

    /// 发送 JSON-RPC 请求
    pub async fn send_request(&mut self, req: &JsonRpcRequest) -> Result<()> {
        let line = serde_json::to_string(req)?;
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// 读取 JSON-RPC 响应
    pub async fn read_response(&mut self) -> Result<JsonRpcResponse> {
        let mut line = String::new();
        self.buf_reader.read_line(&mut line).await?;
        if line.is_empty() {
            anyhow::bail!("Connection closed by server");
        }
        let resp: JsonRpcResponse = serde_json::from_str(&line.trim())?;
        Ok(resp)
    }

    /// 获取 Agent ID
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// 读取 JSON-RPC 请求（Orchestrator 可能主动发送任务）
    pub async fn read_request(&mut self) -> Result<JsonRpcRequest> {
        let mut line = String::new();
        self.buf_reader.read_line(&mut line).await?;
        if line.is_empty() {
            anyhow::bail!("Connection closed by server");
        }
        let req: JsonRpcRequest = serde_json::from_str(&line.trim())?;
        Ok(req)
    }

    /// 发送原始 JSON 字符串（用于发送纯 JSON-RPC 响应/消息）
    pub async fn send_raw(&mut self, json: &str) -> Result<()> {
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }
}
// ===================================================================
// UDS 流（独立的读写句柄）
// ===================================================================

/// UDS 流 — 用于处理已建立的连接
pub struct UdsStream {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
}

impl UdsStream {
    /// 读取一个 JSON-RPC 请求（按行分割）
    pub async fn read_request(&mut self) -> Result<JsonRpcRequest> {
        let mut reader = BufReader::new(&mut self.reader);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.is_empty() {
            anyhow::bail!("Connection closed");
        }
        let req: JsonRpcRequest = serde_json::from_str(&line.trim())?;
        Ok(req)
    }

    /// 发送一个 JSON-RPC 响应
    pub async fn send_response(&mut self, resp: &JsonRpcResponse) -> Result<()> {
        let line = serde_json::to_string(resp)?;
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// 发送一个 JSON-RPC 请求（供 Orchestrator 向 Agent 派发任务）
    pub async fn send_request(&mut self, req: &JsonRpcRequest) -> Result<()> {
        let line = serde_json::to_string(req)?;
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_uds_server_bind_and_cleanup() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let server = UdsServer::bind(&socket_path).await.unwrap();
        assert!(socket_path.exists());
        assert_eq!(server.connection_count(), 0);

        server.cleanup().await;
        assert!(!socket_path.exists());
    }
}
