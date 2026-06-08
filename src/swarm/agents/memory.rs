// src/swarm/agents/memory.rs
// 🧠 Memory Agent — 后台记忆管理 Agent
//
// Memory Agent 是一个非交互式 Agent，通过 UDS 与 Orchestrator 通信。
// 职责：
// 1. 自动记忆提取 — 从对话上下文中提取重要信息
// 2. 记忆检索 — 提供向量检索服务
// 3. 记忆管理 — 保存、删除、统计
// 4. 心跳维护 — 定期向 Orchestrator 报告健康状态

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::Mutex as TokioMutex;
use tokio::time::interval;

use crate::memory::manager::MemoryManager;
use crate::memory::types::MemorySource;
use crate::swarm::heartbeat::create_heartbeat_request;
use crate::swarm::rpc::JsonRpcRequest;
use crate::swarm::transport::{UdsClient, default_socket_path};

/// Memory Agent — 记忆管理 Agent
pub struct MemoryAgent {
    /// Agent ID
    agent_id: String,
    /// UDS 客户端（连接到 Orchestrator），用 Arc<Mutex> 共享给心跳任务
    client: Option<Arc<TokioMutex<UdsClient>>>,
    /// MemoryManager 实例
    memory_manager: Arc<TokioMutex<MemoryManager>>,
    /// 是否正在运行
    running: bool,
}

impl MemoryAgent {
    /// 创建新的 Memory Agent
    pub fn new(memory_manager: MemoryManager) -> Self {
        Self {
            agent_id: format!("memory-{}", std::process::id()),
            client: None,
            memory_manager: Arc::new(TokioMutex::new(memory_manager)),
            running: false,
        }
    }

    /// 连接到 Orchestrator
    pub async fn connect(&mut self, orchestrator_socket: Option<PathBuf>) -> Result<()> {
        let socket = orchestrator_socket.unwrap_or_else(default_socket_path);
        eprintln!("🧠 Memory Agent 连接到 Orchestrator @ {:?}", socket);

        let client = UdsClient::connect(&socket, &self.agent_id)
            .await
            .context(format!("无法连接到 Orchestrator (socket: {:?})", socket))?;

        eprintln!("🧠 Memory Agent '{}' 已注册到蜂群", self.agent_id);

        self.client = Some(Arc::new(TokioMutex::new(client)));
        Ok(())
    }

    /// 运行 Memory Agent 主循环
    pub async fn run(&mut self) -> Result<()> {
        self.running = true;
        eprintln!("🧠 Memory Agent 主循环已启动");

        // 启动心跳任务
        let agent_id = self.agent_id.clone();
        let client_arc = self.client.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(15));
            loop {
                ticker.tick().await;
                if let Some(ref client) = client_arc {
                    let mut client = client.lock().await;
                    let hb = create_heartbeat_request(&agent_id);
                    if let Err(e) = client.send_request(&hb).await {
                        eprintln!("🧠 [Heartbeat] 发送失败: {}", e);
                    }
                }
            }
        });

        // 启动后台维护任务：每 30 秒执行一次记忆合并和清理
        let mgr_arc = self.memory_manager.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;
                let mut mgr = mgr_arc.lock().await;
                let stats = mgr.stats();
                if stats.total_entries > 0 {
                    // 只保留重要性 >= 0.2 的记忆
                    let entries = mgr.list(1000);
                    let mut removed = 0;
                    for entry in &entries {
                        if entry.importance < 0.2 {
                            if mgr.forget(&entry.id) {
                                removed += 1;
                            }
                        }
                    }
                    if removed > 0 {
                        eprintln!(
                            "🧠 [维护] 清理了 {} 条低重要性记忆 (threshold=0.2)",
                            removed
                        );
                    }
                    // 触发 compact
                    let _ = mgr.flush();
                }
            }
        });

        // 启动后台自动合并任务：每 60 秒执行一次相似记忆合并
        let mgr_merge = self.memory_manager.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;
                let mut mgr = mgr_merge.lock().await;
                let entries = mgr.list(500);
                if entries.len() < 2 {
                    continue;
                }
                let mut merged = 0;
                for i in 0..entries.len() {
                    for j in (i + 1)..entries.len() {
                        // 检查内容相似度：如果内容完全相同或一方包含另一方，则合并
                        let content_i = entries[i].content.trim();
                        let content_j = entries[j].content.trim();
                        if content_i == content_j
                            || (content_i.len() > 20
                                && content_j.len() > 20
                                && (content_i.contains(content_j) || content_j.contains(content_i)))
                        {
                            // 保留重要性更高的那条
                            if entries[i].importance >= entries[j].importance {
                                mgr.forget(&entries[j].id);
                            } else {
                                mgr.forget(&entries[i].id);
                            }
                            merged += 1;
                            if merged >= 10 {
                                break;
                            }
                        }
                    }
                    if merged >= 10 {
                        break;
                    }
                }
                if merged > 0 {
                    eprintln!("🧠 [维护] 合并了 {} 条相似记忆", merged);
                    let _ = mgr.flush();
                }
            }
        });

        // 主循环：等待处理任务
        while self.running {
            if let Some(ref client_arc) = self.client {
                let mut client = client_arc.lock().await;
                match client.read_request().await {
                    Ok(request) => {
                        let _method = request.method.clone();
                        drop(client); // release lock before handling
                        self.handle_request(request).await;
                    }
                    Err(e) => {
                        drop(client);
                        eprintln!("🧠 读取请求失败: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        Ok(())
    }

    /// 处理收到的请求
    async fn handle_request(&self, request: JsonRpcRequest) {
        match request.method.as_str() {
            "memory_save" => {
                let content = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let importance = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("importance"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5) as f32;
                let tags: Vec<String> = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("tags"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let mut mgr = self.memory_manager.lock().await;
                match mgr
                    .save(&content, &tags, MemorySource::Manual, importance)
                    .await
                {
                    Ok(id) => {
                        eprintln!("🧠 记忆已保存: id={}", id);
                        if let Some(ref client_arc) = self.client {
                            let mut client = client_arc.lock().await;
                            let resp = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": request.id,
                                "result": {
                                    "success": true,
                                    "memory_id": id,
                                    "content": content,
                                }
                            });
                            let _ = client
                                .send_raw(&serde_json::to_string(&resp).unwrap())
                                .await;
                        }
                    }
                    Err(e) => {
                        eprintln!("🧠 记忆保存失败: {}", e);
                    }
                }
            }
            "memory_search" => {
                let query = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("query"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let top_k = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("top_k"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;

                let mgr = self.memory_manager.lock().await;
                match mgr.search_similar(&query, top_k).await {
                    Ok(results) => {
                        if let Some(ref client_arc) = self.client {
                            let mut client = client_arc.lock().await;
                            let memories: Vec<serde_json::Value> = results
                                .iter()
                                .map(|m| {
                                    serde_json::json!({
                                        "id": m.record.id,
                                        "content": m.record.content,
                                        "importance": m.record.importance,
                                        "tags": m.record.tags,
                                        "score": m.score,
                                        "source": m.record.source,
                                    })
                                })
                                .collect();
                            let resp = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": request.id,
                                "result": {
                                    "success": true,
                                    "results": memories,
                                    "count": memories.len(),
                                }
                            });
                            let _ = client
                                .send_raw(&serde_json::to_string(&resp).unwrap())
                                .await;
                        }
                    }
                    Err(e) => {
                        eprintln!("🧠 记忆搜索失败: {}", e);
                    }
                }
            }
            "memory_forget" => {
                let memory_id = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let mut mgr = self.memory_manager.lock().await;
                if mgr.forget(&memory_id) {
                    eprintln!("🧠 记忆已删除: id={}", memory_id);
                    if let Some(ref client_arc) = self.client {
                        let mut client = client_arc.lock().await;
                        let resp = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request.id,
                            "result": {
                                "success": true,
                                "message": format!("记忆 {} 已删除", memory_id),
                            }
                        });
                        let _ = client
                            .send_raw(&serde_json::to_string(&resp).unwrap())
                            .await;
                    }
                } else {
                    eprintln!("🧠 记忆删除失败: id={} 不存在", memory_id);
                }
            }
            "memory_stats" => {
                let mgr = self.memory_manager.lock().await;
                let stats = mgr.stats();
                if let Some(ref client_arc) = self.client {
                    let mut client = client_arc.lock().await;
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": request.id,
                        "result": {
                            "success": true,
                            "stats": {
                                "total_entries": stats.total_entries,
                                "vector_dim": stats.vector_dim,
                                "last_compaction": stats.last_compaction,
                            },
                        }
                    });
                    let _ = client
                        .send_raw(&serde_json::to_string(&resp).unwrap())
                        .await;
                }
            }
            "shutdown" => {
                eprintln!("🧠 Memory Agent 收到关闭信号");
            }
            other => {
                eprintln!("🧠 未知方法: {}", other);
            }
        }
    }

    /// 停止 Memory Agent
    pub fn stop(&mut self) {
        self.running = false;
    }
}
