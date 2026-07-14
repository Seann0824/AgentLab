use std::path::PathBuf;

/// tracing subscriber 初始化守卫。
///
/// Release 模式下需要持有 `WorkerGuard`，避免非阻塞文件 appender 在初始化后被丢弃。
#[derive(Debug)]
pub struct TracingGuard {
    #[cfg(not(debug_assertions))]
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

/// 初始化 tracing subscriber。
///
/// - Debug 构建：输出到控制台（带颜色）。
/// - Release 构建：输出到滚动文件 `logs/agent-lab.YYYY-MM-DD.log`。
///
/// 日志级别优先从 `RUST_LOG` 环境变量读取，否则默认 `INFO`。
/// 若 subscriber 已被其他组件（如 Tauri DevTools）初始化，则返回 `None` 且不 panic。
pub fn init_tracing(_log_dir: Option<PathBuf>) -> Option<TracingGuard> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    #[cfg(debug_assertions)]
    {
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(false)
            .with_line_number(true)
            .with_ansi(true)
            .finish();

        match tracing::subscriber::set_global_default(subscriber) {
            Ok(_) => Some(TracingGuard {}),
            Err(_) => {
                eprintln!("[init_tracing] tracing subscriber already initialized, skipping");
                None
            }
        }
    }

    #[cfg(not(debug_assertions))]
    {
        let log_dir = log_dir.unwrap_or_else(|| PathBuf::from("logs"));
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            eprintln!(
                "[init_tracing] failed to create log dir {:?}: {}",
                log_dir, e
            );
        }

        let file_appender = tracing_appender::rolling::daily(&log_dir, "agent-lab.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(false)
            .with_line_number(true)
            .finish();

        match tracing::subscriber::set_global_default(subscriber) {
            Ok(_) => Some(TracingGuard { _guard: guard }),
            Err(_) => {
                eprintln!("[init_tracing] tracing subscriber already initialized, skipping");
                None
            }
        }
    }
}
