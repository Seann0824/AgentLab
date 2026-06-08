// src/tools/swarm_ctl.rs
// SwarmCtl — 蜂群控制工具，用于查询和管理多 Agent 蜂群
//
// 设计文档: docs/designs/multi-agent-swarm-architecture.md

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::swarm::registry::{AgentType, SwarmRegistry};
use crate::tools::types::{Tool, ToolEvent, ToolStream};

/// 蜂群控制工具 — 查询和管理多 Agent 蜂群
pub struct SwarmCtl {
    pub registry: Option<SwarmRegistry>,
}

impl SwarmCtl {
    pub fn new(registry: Option<SwarmRegistry>) -> Self {
        Self { registry }
    }
}

impl Tool for SwarmCtl {
    fn name(&self) -> &str {
        "swarm_ctl"
    }

    fn description(&self) -> &str {
        "蜂群控制工具：查询和管理多 Agent 蜂群状态。支持 status（蜂群概览）、list（列出所有 Agent）、query（按类型/状态查询）"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "swarm_ctl",
                "description": "蜂群控制工具：查询和管理多 Agent 蜂群状态",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "操作类型：status（蜂群概览）、list（列出所有 Agent）、query（按类型查询）",
                            "enum": ["status", "list", "query"]
                        },
                        "agent_type": {
                            "type": "string",
                            "description": "查询时指定的 Agent 类型：orchestrator/memory/general/verifier",
                            "enum": ["orchestrator", "memory", "general", "verifier"]
                        }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                }
            }
        })
    }

    fn execute(&self, args: serde_json::Value) -> ToolStream {
        let (tx, rx) = mpsc::channel(1);

        let registry = self.registry.clone();

        tokio::spawn(async move {
            let action = args["action"].as_str().unwrap_or("status");

            let result = match action {
                "list" => {
                    if let Some(registry) = &registry {
                        let agents: Vec<serde_json::Value> = registry.all_agents()
                            .iter()
                            .map(|a| serde_json::json!({
                                "agent_id": a.agent_id,
                                "agent_type": a.agent_type.as_str(),
                                "status": a.status.as_str(),
                                "hostname": a.hostname,
                            }))
                            .collect();
                        serde_json::json!({
                            "success": true,
                            "total_agents": agents.len(),
                            "agents": agents,
                        })
                    } else {
                        serde_json::json!({
                            "success": false,
                            "message": "蜂群注册表未初始化（当前为非 Orchestrator 模式）",
                            "hint": "启动时使用默认模式（不带 --agent-type 参数）以激活蜂群功能"
                        })
                    }
                }
                "query" => {
                    let agent_type_str = args["agent_type"].as_str().unwrap_or("general");
                    let agent_type = AgentType::from_str(agent_type_str);

                    if let Some(registry) = &registry {
                        let agents: Vec<serde_json::Value> = registry.query_by_type(&agent_type)
                            .iter()
                            .map(|a| serde_json::json!({
                                "agent_id": a.agent_id,
                                "status": a.status.as_str(),
                                "hostname": a.hostname,
                            }))
                            .collect();
                        serde_json::json!({
                            "success": true,
                            "agent_type": agent_type_str,
                            "count": agents.len(),
                            "agents": agents,
                        })
                    } else {
                        serde_json::json!({
                            "success": false,
                            "message": "蜂群注册表未初始化",
                        })
                    }
                }
                _ => {
                    // status — 蜂群概览
                    if let Some(registry) = &registry {
                        let all = registry.all_agents();
                        let online = registry.online_count();
                        serde_json::json!({
                            "success": true,
                            "swarm_status": {
                                "total_agents": all.len(),
                                "online": online,
                                "offline": all.len() - online,
                                "agents": all.iter().map(|a| serde_json::json!({
                                    "id": a.agent_id,
                                    "type": a.agent_type.as_str(),
                                    "status": a.status.as_str(),
                                })).collect::<Vec<_>>(),
                            },
                            "message": format!("🐝 蜂群状态：共 {} 个 Agent，{} 个在线", all.len(), online),
                        })
                    } else {
                        serde_json::json!({
                            "success": true,
                            "swarm_status": {
                                "total_agents": 0,
                                "online": 0,
                                "offline": 0,
                                "agents": [],
                            },
                            "message": "🐝 蜂群模块已加载（SwarmRegistry 未激活，运行 Orchestrator 模式后生效）",
                            "available_modules": ["transport (UDS)", "rpc (JSON-RPC 2.0)", "registry", "heartbeat"],
                        })
                    }
                }
            };

            let _ = tx.send(ToolEvent::Done(result)).await;
        });

        Box::pin(ReceiverStream::new(rx))
    }
}
