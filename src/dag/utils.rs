// src/dag/utils.rs
// DAG 编排系统 — 工具函数

use std::time::{SystemTime, UNIX_EPOCH};

/// 获取当前 Unix 时间戳（秒，带小数）
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
