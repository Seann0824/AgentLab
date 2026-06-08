use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn format_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    // 转换为 ISO 8601 格式
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        1970 + (days / 365) as u32,     // 近似年份
        ((days % 365) / 30 + 1) as u32, // 近似月份
        ((days % 365) % 30 + 1) as u32, // 近似日
        hours,
        minutes,
        seconds,
        millis
    )
}

// ─── 测试 ───────────────────────────────────────────────
