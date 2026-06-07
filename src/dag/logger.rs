// src/dag/logger.rs
// 可视化日志输出 — ANSI 彩色 Pipeline 执行过程
//
// 输出到 stderr（不干扰 stdout 的数据输出）
// 使用 emoji 和颜色区分不同类型的事件

use crate::dag::engine::DAGEngine;
use crate::dag::types::NodeStatus;
use std::collections::HashMap;
use std::time::Instant;

/// ANSI 颜色常量
mod color {
    pub const RESET: &str = "\x1b[0m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const RED: &str = "\x1b[31m";
    pub const BLUE: &str = "\x1b[34m";
    pub const CYAN: &str = "\x1b[36m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const GRAY: &str = "\x1b[90m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
}

/// Pipeline 可视化日志器
pub struct PipelineLogger {
    /// Pipeline ID
    pipeline_id: String,
    /// 开始时间
    start_time: Instant,
    /// 节点状态缓存（用于计算变更）
    node_states: HashMap<String, NodeStatus>,
    /// 是否已输出标题
    header_printed: bool,
}

impl PipelineLogger {
    /// 创建新的 Pipeline 日志器
    pub fn new(pipeline_id: impl Into<String>) -> Self {
        Self {
            pipeline_id: pipeline_id.into(),
            start_time: Instant::now(),
            node_states: HashMap::new(),
            header_printed: false,
        }
    }

    /// 输出 Pipeline 启动信息
    pub fn pipeline_started(&mut self, total_nodes: usize) {
        self.header_printed = true;
        eprintln!();
        eprintln!("{}═══════════════════════════════════════════{}", color::CYAN, color::RESET);
        eprintln!("{}🔷 Pipeline: {}{}{}",
            color::BOLD, color::CYAN, self.pipeline_id, color::RESET);
        eprintln!("{}   节点数: {}{}", color::GRAY, total_nodes, color::RESET);
        eprintln!("{}═══════════════════════════════════════════{}", color::CYAN, color::RESET);
        eprintln!();
    }

    /// 输出节点状态变更
    pub fn node_status_changed(&mut self, node_id: &str, old_status: &NodeStatus, new_status: &NodeStatus) {
        if !self.header_printed {
            return;
        }

        let elapsed = self.start_time.elapsed();
        let timestamp = format!("[{:>4}.{:02}s]", elapsed.as_secs(), elapsed.subsec_millis() / 10);

        // 根据新状态选择图标和颜色
        let (icon, status_color) = match new_status {
            NodeStatus::Pending => ("⏳", color::GRAY),
            NodeStatus::Ready => ("✅", color::GREEN),
            NodeStatus::Working => ("⚙️ ", color::BLUE),
            NodeStatus::Reviewing => ("🔍", color::MAGENTA),
            NodeStatus::Approved => ("👍", color::GREEN),
            NodeStatus::Rejected { .. } => ("🔄", color::YELLOW),
            NodeStatus::Completed => ("✅", color::GREEN),
            NodeStatus::Failed { .. } => ("❌", color::RED),
            NodeStatus::Skipped { .. } => ("⏭️ ", color::YELLOW),
        };

        let status_text = format!("{:?}", new_status);
        eprintln!("{} {}{} {}{} → {}{}",
            timestamp,
            icon,
            color::BOLD,
            node_id,
            color::DIM,
            status_color,
            status_text,
        );
        eprintln!("{}", color::RESET);
    }

    /// 输出节点执行摘要（完成后）
    pub fn node_completed(&mut self, node_id: &str, duration_secs: f64, retries: u32) {
        if !self.header_printed {
            return;
        }
        let retry_text = if retries > 0 {
            format!(" (重试 {} 次)", retries)
        } else {
            String::new()
        };
        eprintln!("{}  ├─ 完成: {} ({:.1}s{})",
            color::GREEN, node_id, duration_secs, retry_text);
        eprintln!("{}", color::RESET);
    }

    /// 输出节点失败信息
    pub fn node_failed(&mut self, node_id: &str, error: &str) {
        if !self.header_printed {
            return;
        }
        eprintln!("{}  ├─ ❌ {}: {}",
            color::RED, node_id, error);
        eprintln!("{}", color::RESET);
    }

    /// 输出 Pipeline 完成摘要
    pub fn pipeline_completed(&mut self, engine: &DAGEngine) {
        if !self.header_printed {
            return;
        }
        let elapsed = self.start_time.elapsed();
        let is_success = engine.nodes.values().all(|n| n.status == NodeStatus::Completed);
        let total = engine.nodes.len();
        let completed = engine.nodes.values().filter(|n| n.status == NodeStatus::Completed).count();
        let failed = engine.nodes.values().filter(|n| n.status.is_failed()).count();

        eprintln!();
        eprintln!("{}═══════════════════════════════════════════{}", color::CYAN, color::RESET);
        if is_success {
            eprintln!("{}✅ Pipeline 执行成功{}{}",
                color::GREEN, color::BOLD, color::RESET);
        } else {
            eprintln!("{}❌ Pipeline 执行完成（部分失败）{}",
                color::RED, color::RESET);
        }
        eprintln!("{}   耗时: {:.1}s   完成: {}/{}   失败: {}",
            color::GRAY, elapsed.as_secs_f64(), completed, total, failed);
        eprintln!("{}═══════════════════════════════════════════{}", color::CYAN, color::RESET);
        eprintln!();
    }

    /// 输出进度条（简易文本版）
    pub fn print_progress(&self, completed: usize, total: usize) {
        let bar_width = 30;
        let filled = if total > 0 { completed * bar_width / total } else { 0 };
        let empty = bar_width - filled;

        let fill_char = "█";
        let empty_char = "░";

        eprintln!("\r{}进度: |{}{}| {}/{}",
            color::CYAN,
            fill_char.repeat(filled),
            empty_char.repeat(empty),
            completed,
            total,
        );
    }
}

/// 从 engine 创建日志器并输出当前状态摘要
pub fn log_engine_status(engine: &DAGEngine) {
    let total = engine.nodes.len();
    let completed = engine.nodes.values().filter(|n| n.status == NodeStatus::Completed).count();
    let failed = engine.nodes.values().filter(|n| n.status.is_failed()).count();
    let running = engine.nodes.values().filter(|n| {
        matches!(n.status, NodeStatus::Working | NodeStatus::Reviewing)
    }).count();
    let pending = engine.nodes.values().filter(|n| {
        matches!(n.status, NodeStatus::Pending | NodeStatus::Ready)
    }).count();

    eprintln!("{}📊 Pipeline [{}] 状态:",
        color::BOLD, engine.pipeline.id);
    eprintln!("{}   完成: {}/{}",
        color::GREEN, completed, total);
    if failed > 0 {
        eprintln!("{}   失败: {}",
            color::RED, failed);
    }
    if running > 0 {
        eprintln!("{}   运行中: {}",
            color::BLUE, running);
    }
    if pending > 0 {
        eprintln!("{}   等待: {}",
            color::GRAY, pending);
    }
    eprintln!("{}", color::RESET);
}
