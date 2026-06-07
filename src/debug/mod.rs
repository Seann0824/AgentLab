// src/debug/mod.rs
//
// ⭐ 全局 Debug 模块
//
// 提供一个全局原子布尔标志，控制当前会话的 debug 模式。
// 当 debug 开启时，代码中所有 debug 条件判断的代码块都会执行。
//
// 使用方式：
//   // 在任意代码中检查 debug 标志
//   if debug::is_enabled() {
//       eprintln!("[DEBUG] 某个调试信息");
//   }
//
//   // 使用宏简化（推荐）
//   debug::debug!("这是调试输出: {}", some_var);
//
//   // CLI 切换
//   > /debug on
//   > /debug off
//   > /debug status

use std::sync::atomic::{AtomicBool, Ordering};

/// 全局 debug 标志
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// 检查 debug 是否开启
pub fn is_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// 开启 debug 模式
pub fn enable() {
    DEBUG_ENABLED.store(true, Ordering::Relaxed);
}

/// 关闭 debug 模式
pub fn disable() {
    DEBUG_ENABLED.store(false, Ordering::Relaxed);
}

/// 切换 debug 模式，返回切换后的状态
pub fn toggle() -> bool {
    let current = DEBUG_ENABLED.load(Ordering::Relaxed);
    let new = !current;
    DEBUG_ENABLED.store(new, Ordering::Relaxed);
    new
}

/// 获取当前状态的描述字符串
pub fn status_text() -> String {
    if is_enabled() {
        "🟢 debug 模式已开启 — 所有 debug 代码都会执行".to_string()
    } else {
        "🔴 debug 模式已关闭 — debug 代码不会执行".to_string()
    }
}

/// ⭐ debug! 宏 —— 仅在 debug 开启时输出到 stderr
///
/// 用法：
///   debug::debug!("当前变量值: {}", value);
///   debug::debug!("some message");
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::debug::is_enabled() {
            eprintln!($($arg)*);
        }
    };
}

/// ⭐ debug_fn! 宏 —— 仅在 debug 开启时执行一个代码块
///
/// 用法：
///   debug_fn!({
///       let detailed_info = expensive_computation();
///       eprintln!("[DEBUG] {}", detailed_info);
///   });
#[macro_export]
macro_rules! debug_fn {
    ($block:block) => {
        if $crate::debug::is_enabled() {
            $block
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_default_off() {
        assert!(!is_enabled());
    }

    #[test]
    fn test_debug_enable_disable() {
        disable();
        assert!(!is_enabled());
        enable();
        assert!(is_enabled());
        disable();
        assert!(!is_enabled());
    }

    #[test]
    fn test_debug_toggle() {
        disable();
        assert!(toggle());  // false -> true
        assert!(is_enabled());
        assert!(!toggle()); // true -> false
        assert!(!is_enabled());
    }

    #[test]
    fn test_debug_status_text() {
        disable();
        assert!(status_text().contains("关闭"));
        enable();
        assert!(status_text().contains("开启"));
    }
}
