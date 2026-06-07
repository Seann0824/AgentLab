// src/tools/dag_tools/store.rs
// PipelineStore — 全局 Pipeline 存储

use std::sync::{LazyLock, Mutex};

use crate::dag::engine::DAGEngine;
use crate::dag::pipeline::PipelineDef;

/// 全局 Pipeline 存储
static PIPELINE_STORE: LazyLock<Mutex<Vec<PipelineDef>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// 全局 DAG 引擎存储
static ENGINE_STORE: LazyLock<Mutex<Vec<DAGEngine>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// 注册一个新的 Pipeline
pub fn register_pipeline(pipeline: PipelineDef) -> Result<(), String> {
    // 验证 Pipeline
    pipeline.validate().map_err(|e| e.to_string())?;

    let mut store = PIPELINE_STORE.lock().map_err(|e| e.to_string())?;
    // 检查是否已存在同名 Pipeline
    if store.iter().any(|p| p.id == pipeline.id) {
        return Err(format!("Pipeline '{}' 已存在", pipeline.id));
    }
    store.push(pipeline);
    Ok(())
}

/// 获取所有已注册的 Pipeline
pub fn list_pipelines() -> Result<Vec<PipelineDef>, String> {
    let store = PIPELINE_STORE.lock().map_err(|e| e.to_string())?;
    Ok(store.clone())
}

/// 通过 ID 获取 Pipeline
pub fn get_pipeline(id: &str) -> Result<PipelineDef, String> {
    let store = PIPELINE_STORE.lock().map_err(|e| e.to_string())?;
    store
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| format!("Pipeline '{}' 未找到", id))
}

/// 创建一个新的 DAGEngine（但不启动执行）
pub fn create_engine(pipeline_id: &str) -> Result<DAGEngine, String> {
    let pipeline = get_pipeline(pipeline_id)?;
    DAGEngine::new(pipeline).map_err(|e| e.to_string())
}

/// 保存 DAGEngine 状态
pub fn save_engine(engine: DAGEngine) -> Result<(), String> {
    let mut store = ENGINE_STORE.lock().map_err(|e| e.to_string())?;
    let id = engine.pipeline.id.clone();
    // 替换已存在的引擎
    if let Some(pos) = store.iter().position(|e| e.pipeline.id == id) {
        store[pos] = engine;
    } else {
        store.push(engine);
    }
    Ok(())
}

/// 获取引擎状态
pub fn get_engine(pipeline_id: &str) -> Result<DAGEngine, String> {
    let store = ENGINE_STORE.lock().map_err(|e| e.to_string())?;
    store
        .iter()
        .find(|e| e.pipeline.id == pipeline_id)
        .cloned()
        .ok_or_else(|| format!("引擎 '{}' 未找到", pipeline_id))
}
