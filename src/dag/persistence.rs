// src/dag/persistence.rs
// Pipeline 断点续跑 — 持久化引擎状态到磁盘 JSON 文件
//
// 功能：
// - 保存 DAGEngine 状态为 JSON checkpoint
// - 从 JSON checkpoint 恢复 DAGEngine
// - 自动管理 checkpoint 目录
// - 节点完成后自动保存

use std::fs;
use std::path::{Path, PathBuf};

use crate::dag::engine::DAGEngine;
use crate::dag::node::NodeInstance;
use crate::dag::pipeline::PipelineDef;
use crate::dag::types::{
    CheckResult, DAGEvent, DAGResult, NodeLog, NodeStatus, PipelineStatus, ReviewResult,
};

/// Checkpoint 管理器
pub struct CheckpointManager {
    /// checkpoint 存储根目录
    root_dir: PathBuf,
}

impl CheckpointManager {
    /// 创建新的 Checkpoint 管理器
    ///
    /// # 参数
    /// * `base_dir` — 基础目录（如 `.agent/checkpoints/`）
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let root_dir = base_dir.into();
        Self { root_dir }
    }

    /// 获取 Pipeline 的 checkpoint 目录
    fn pipeline_dir(&self, pipeline_id: &str) -> PathBuf {
        // 对 pipeline_id 做安全处理，避免路径问题
        let safe_id = pipeline_id.replace('/', "_").replace('\\', "_");
        self.root_dir.join(&safe_id)
    }

    /// 获取最新的 checkpoint 文件路径
    fn latest_file(&self, pipeline_id: &str) -> PathBuf {
        self.pipeline_dir(pipeline_id).join("latest.json")
    }

    /// 获取历史 checkpoint 文件路径
    fn history_file(&self, pipeline_id: &str, seq: u32) -> PathBuf {
        self.pipeline_dir(pipeline_id).join(format!("seq_{:04}.json", seq))
    }

    /// 保存引擎状态为 checkpoint
    ///
    /// 将 DAGEngine 序列化为 JSON，同时保存到：
    /// 1. `latest.json` — 最新的 checkpoint（覆盖）
    /// 2. `seq_{NNNN}.json` — 历史 checkpoint（追加，用于回放）
    pub fn save_checkpoint(&self, engine: &DAGEngine) -> DAGResult<PathBuf> {
        let dir = self.pipeline_dir(&engine.pipeline.id);
        fs::create_dir_all(&dir)
            .map_err(|e| crate::dag::types::DAGError::Internal(
                format!("创建 checkpoint 目录失败: {}", e)
            ))?;

        // 构建可序列化的 Checkpoint 数据
        let checkpoint = CheckpointData::from_engine(engine);

        // 序列化
        let json = serde_json::to_string_pretty(&checkpoint)
            .map_err(|e| crate::dag::types::DAGError::Internal(
                format!("序列化 checkpoint 失败: {}", e)
            ))?;

        // 写入最新文件
        let latest_path = self.latest_file(&engine.pipeline.id);
        fs::write(&latest_path, &json)
            .map_err(|e| crate::dag::types::DAGError::Internal(
                format!("写入 checkpoint 失败: {}", e)
            ))?;

        // 写入历史文件（每 5 个节点完成保存一次历史）
        let completed_count = engine.nodes.values()
            .filter(|n| n.status == NodeStatus::Completed)
            .count();
        let history_path = self.history_file(&engine.pipeline.id, completed_count as u32);
        if !history_path.exists() {
            let _ = fs::write(&history_path, &json);
        }

        Ok(latest_path)
    }

    /// 从最新的 checkpoint 恢复引擎状态
    pub fn load_latest(&self, pipeline_id: &str) -> DAGResult<DAGEngine> {
        let path = self.latest_file(pipeline_id);
        if !path.exists() {
            return Err(crate::dag::types::DAGError::Internal(
                format!("checkpoint 不存在: {}", path.display())
            ));
        }

        let json = fs::read_to_string(&path)
            .map_err(|e| crate::dag::types::DAGError::Internal(
                format!("读取 checkpoint 失败: {}", e)
            ))?;

        let checkpoint: CheckpointData = serde_json::from_str(&json)
            .map_err(|e| crate::dag::types::DAGError::Internal(
                format!("反序列化 checkpoint 失败: {}", e)
            ))?;

        checkpoint.into_engine()
    }

    /// 检查是否存在可恢复的 checkpoint
    pub fn has_checkpoint(&self, pipeline_id: &str) -> bool {
        self.latest_file(pipeline_id).exists()
    }

    /// 列出所有有 checkpoint 的 Pipeline
    pub fn list_checkpoints(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.root_dir) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let dir_name = e.file_name().into_string().ok()?;
                let latest = e.path().join("latest.json");
                if latest.exists() { Some(dir_name) } else { None }
            })
            .collect()
    }
}

// =====================================================================
// CheckpointData — 可序列化的 checkpoint 数据
// =====================================================================

/// Checkpoint 序列化数据结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CheckpointData {
    /// Pipeline 定义（完整保留）
    pipeline: PipelineDef,
    /// 节点运行时状态快照
    nodes: Vec<NodeSnapshot>,
    /// Pipeline 执行状态
    status: String,
    /// 执行顺序
    execution_order: Vec<String>,
    /// 累计事件（最近 100 条）
    events: Vec<DAGEvent>,
    /// 保存时间戳
    saved_at: f64,
    /// Checkpoint 格式版本
    version: u32,
}

/// 节点状态快照（可序列化的精简版 NodeInstance）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NodeSnapshot {
    node_id: String,
    status: String,
    status_detail: Option<serde_json::Value>,
    input: Option<serde_json::Value>,
    worker_output: Option<String>,
    review_passed: Option<bool>,
    review_score: Option<f32>,
    review_feedback: Option<String>,
    review_details: Option<Vec<CheckResult>>,
    final_output: Option<serde_json::Value>,
    retry_count: u32,
    started_at: Option<f64>,
    completed_at: Option<f64>,
    log_count: usize,
}

impl CheckpointData {
    /// 从 DAGEngine 构建 CheckpointData
    fn from_engine(engine: &DAGEngine) -> Self {
        let nodes = engine.nodes.values().map(|n| {
            let (status, status_detail) = serialize_status(&n.status);
            let (review_passed, review_score, review_feedback, review_details) = match &n.review_result {
                Some(r) => (Some(r.passed), r.score, Some(r.feedback.clone()), Some(r.details.clone())),
                None => (None, None, None, None),
            };

            NodeSnapshot {
                node_id: n.node_id.clone(),
                status,
                status_detail,
                input: n.input.clone(),
                worker_output: n.worker_output.clone(),
                review_passed,
                review_score,
                review_feedback,
                review_details,
                final_output: n.final_output.clone(),
                retry_count: n.retry_count,
                started_at: n.started_at,
                completed_at: n.completed_at,
                log_count: n.logs.len(),
            }
        }).collect();

        let saved_at = crate::dag::utils::now_secs();

        // 仅保留最近 100 条事件
        let events = if engine.events.len() > 100 {
            engine.events[engine.events.len() - 100..].to_vec()
        } else {
            engine.events.clone()
        };

        Self {
            pipeline: engine.pipeline.clone(),
            nodes,
            status: format!("{:?}", engine.status),
            execution_order: engine.execution_order.clone(),
            events,
            saved_at,
            version: 1,
        }
    }

    /// 将 CheckpointData 还原为 DAGEngine
    fn into_engine(self) -> DAGResult<DAGEngine> {
        use std::collections::HashMap;

        let status = deserialize_pipeline_status(&self.status);

        let mut nodes_map = HashMap::new();
        for snapshot in &self.nodes {
            let instance = NodeInstance {
                node_id: snapshot.node_id.clone(),
                status: deserialize_status(&snapshot.status),
                input: snapshot.input.clone(),
                worker_output: snapshot.worker_output.clone(),
                review_result: snapshot.review_passed.map(|passed| ReviewResult {
                    passed,
                    score: snapshot.review_score,
                    feedback: snapshot.review_feedback.clone().unwrap_or_default(),
                    details: snapshot.review_details.clone().unwrap_or_default(),
                }),
                final_output: snapshot.final_output.clone(),
                logs: Vec::new(), // 恢复时不保留日志（占用空间）
                started_at: snapshot.started_at,
                completed_at: snapshot.completed_at,
                retry_count: snapshot.retry_count,
            };
            nodes_map.insert(snapshot.node_id.clone(), instance);
        }

        Ok(DAGEngine {
            pipeline: self.pipeline,
            nodes: nodes_map,
            status,
            execution_order: self.execution_order,
            events: self.events,
            started_at: self.saved_at,
        })
    }
}

// =====================================================================
// 序列化辅助函数
// =====================================================================

/// 将 NodeStatus 序列化为字符串 + 可选详细数据
fn serialize_status(status: &NodeStatus) -> (String, Option<serde_json::Value>) {
    match status {
        NodeStatus::Pending => ("Pending".to_string(), None),
        NodeStatus::Ready => ("Ready".to_string(), None),
        NodeStatus::Working => ("Working".to_string(), None),
        NodeStatus::Reviewing => ("Reviewing".to_string(), None),
        NodeStatus::Approved => ("Approved".to_string(), None),
        NodeStatus::Rejected { retry_count, reason } => (
            "Rejected".to_string(),
            Some(serde_json::json!({ "retry_count": retry_count, "reason": reason })),
        ),
        NodeStatus::Completed => ("Completed".to_string(), None),
        NodeStatus::Failed { error } => (
            "Failed".to_string(),
            Some(serde_json::json!({ "error": error })),
        ),
        NodeStatus::Skipped { reason } => (
            "Skipped".to_string(),
            Some(serde_json::json!({ "reason": reason })),
        ),
    }
}

/// 从字符串反序列化 NodeStatus
fn deserialize_status(s: &str) -> NodeStatus {
    match s {
        "Pending" => NodeStatus::Pending,
        "Ready" => NodeStatus::Ready,
        "Working" => NodeStatus::Working,
        "Reviewing" => NodeStatus::Reviewing,
        "Approved" => NodeStatus::Approved,
        "Rejected" => NodeStatus::Rejected { retry_count: 0, reason: String::new() },
        "Completed" => NodeStatus::Completed,
        "Failed" => NodeStatus::Failed { error: String::new() },
        "Skipped" => NodeStatus::Skipped { reason: String::new() },
        _ => NodeStatus::Pending,
    }
}

/// 从字符串反序列化 PipelineStatus
fn deserialize_pipeline_status(s: &str) -> PipelineStatus {
    match s {
        "Pending" => PipelineStatus::Pending,
        "Running" => PipelineStatus::Running,
        "Completed" => PipelineStatus::Completed,
        "Failed" => PipelineStatus::Failed { failed_nodes: Vec::new() },
        "Cancelled" => PipelineStatus::Cancelled,
        _ => PipelineStatus::Pending,
    }
}
