// src/dag/event_bus.rs
// 事件总线 — 结构化事件发布、订阅与日志记录
//
// 基于 DAGEvent 提供：
// - 回调订阅（节点完成、失败等特定事件）
// - 文件日志记录
// - 事件过滤

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::dag::types::DAGEvent;

/// 事件处理回调
pub type EventCallback = Arc<dyn Fn(&DAGEvent) + Send + Sync>;

/// 事件总线 — 管理事件的分发和记录
#[derive(Clone)]
pub struct EventBus {
    /// 订阅者列表
    subscribers: Arc<Mutex<Vec<EventCallback>>>,
    /// 是否启用文件日志
    file_logging: Arc<Mutex<bool>>,
    /// 日志文件路径
    log_path: Arc<Mutex<Option<String>>>,
}

impl EventBus {
    /// 创建新的事件总线
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
            file_logging: Arc::new(Mutex::new(false)),
            log_path: Arc::new(Mutex::new(None)),
        }
    }

    /// 发布一个事件，通知所有订阅者
    pub async fn publish(&self, event: &DAGEvent) {
        let subscribers = self.subscribers.lock().await;
        for callback in subscribers.iter() {
            callback(event);
        }
    }

    /// 订阅所有事件
    pub async fn subscribe(&self, callback: EventCallback) {
        let mut subscribers = self.subscribers.lock().await;
        subscribers.push(callback);
    }

    /// 启用文件日志
    pub async fn enable_file_logging(&self, path: impl Into<String>) {
        let mut log_path = self.log_path.lock().await;
        *log_path = Some(path.into());
        let mut file_logging = self.file_logging.lock().await;
        *file_logging = true;
    }

    /// 清除所有订阅者
    pub async fn clear_subscribers(&self) {
        let mut subscribers = self.subscribers.lock().await;
        subscribers.clear();
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 创建一个将事件写入文件的事件总线
pub async fn create_event_logger(path: impl Into<String>) -> EventBus {
    let bus = EventBus::new();
    let path = path.into();
    let path_clone = path.clone();

    // 注册文件日志回调
    bus.subscribe(Arc::new(move |event| {
        let event_json = serde_json::to_string(event).unwrap_or_default();
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path_clone)
            .and_then(|file| {
                use std::io::Write;
                let mut file = file;
                writeln!(file, "{}", event_json)
            });
    })).await;

    bus.enable_file_logging(path).await;
    bus
}
