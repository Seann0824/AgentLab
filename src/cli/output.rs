// src/cli/output.rs
//
// ⭐ CLI 输出样式工具模块
//
// 提供统一的、产品级的 CLI 输出格式化能力：
// - 颜色与样式（ANSI 256 色 + 基础色）
// - 图标与徽章（成功/错误/警告/信息）
// - 分区标题与分隔线
// - 表格渲染
// - 加载动画
// - 提示符生成
// - 启动横幅

use std::time::Instant;

// =====================================================================
// 🎨 ANSI 颜色与样式常量
// =====================================================================

pub mod style {
    // 重置
    pub const RESET: &str = "\x1b[0m";

    // 前景色
    pub const FG_BLACK: &str = "\x1b[30m";
    pub const FG_RED: &str = "\x1b[31m";
    pub const FG_GREEN: &str = "\x1b[32m";
    pub const FG_YELLOW: &str = "\x1b[33m";
    pub const FG_BLUE: &str = "\x1b[34m";
    pub const FG_MAGENTA: &str = "\x1b[35m";
    pub const FG_CYAN: &str = "\x1b[36m";
    pub const FG_WHITE: &str = "\x1b[37m";
    pub const FG_BRIGHT_BLACK: &str = "\x1b[90m";
    pub const FG_BRIGHT_RED: &str = "\x1b[91m";
    pub const FG_BRIGHT_GREEN: &str = "\x1b[92m";
    pub const FG_BRIGHT_YELLOW: &str = "\x1b[93m";
    pub const FG_BRIGHT_BLUE: &str = "\x1b[94m";
    pub const FG_BRIGHT_MAGENTA: &str = "\x1b[95m";
    pub const FG_BRIGHT_CYAN: &str = "\x1b[96m";
    pub const FG_BRIGHT_WHITE: &str = "\x1b[97m";

    // 背景色
    pub const BG_RED: &str = "\x1b[41m";
    pub const BG_GREEN: &str = "\x1b[42m";
    pub const BG_YELLOW: &str = "\x1b[43m";
    pub const BG_BLUE: &str = "\x1b[44m";
    pub const BG_MAGENTA: &str = "\x1b[45m";
    pub const BG_CYAN: &str = "\x1b[46m";
    pub const BG_BRIGHT_BLACK: &str = "\x1b[100m";
    pub const BG_BRIGHT_RED: &str = "\x1b[101m";
    pub const BG_BRIGHT_GREEN: &str = "\x1b[102m";
    pub const BG_BRIGHT_YELLOW: &str = "\x1b[103m";
    pub const BG_BRIGHT_CYAN: &str = "\x1b[106m";

    // 文本样式
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";
    pub const UNDERLINE: &str = "\x1b[4m";
    pub const BLINK: &str = "\x1b[5m";
    pub const REVERSE: &str = "\x1b[7m";
    pub const HIDDEN: &str = "\x1b[8m";
    pub const STRIKETHROUGH: &str = "\x1b[9m";

    // 清除
    pub const CLEAR_LINE: &str = "\r\x1b[2K";
    pub const CLEAR_SCREEN: &str = "\x1b[2J";
    pub const CURSOR_UP: &str = "\x1b[1A";
}

// =====================================================================
// 🏷️ 状态徽章
// =====================================================================

/// 成功徽章
pub fn badge_success(msg: &str) -> String {
    format!("{}{} ✅ {}{}{}", style::FG_GREEN, style::BOLD, msg, style::RESET, style::RESET)
}

/// 错误徽章
pub fn badge_error() -> String {
    format!("{}❌{} {}", style::FG_RED, style::RESET, style::BOLD)
}

/// 警告徽章
pub fn badge_warning(msg: &str) -> String {
    format!("{}{} ⚠️ {}{}{}", style::FG_YELLOW, style::BOLD, msg, style::RESET, style::RESET)
}

/// 信息徽章
pub fn badge_info() -> String {
    format!("{}ℹ️{} {}", style::FG_BLUE, style::RESET, style::BOLD)
}

/// 完成徽章
pub fn badge_done() -> String {
    format!("{}✨{} {}", style::FG_BRIGHT_MAGENTA, style::RESET, style::BOLD)
}

// =====================================================================
// 📐 分隔线与分区标题
// =====================================================================

/// 绘制等宽分隔线
pub fn separator(width: usize) -> String {
    let bar = "━".repeat(width);
    format!("{}{}{}", style::FG_BRIGHT_BLACK, bar, style::RESET)
}

/// 带标题的分区头部
pub fn section(title: &str, icon: &str) -> String {
    let width = 60usize;
    let title_part = format!(" {} {} ", icon, title);
    let bar_total = width.saturating_sub(title_part.len());
    let left = bar_total / 2;
    let right = bar_total - left;
    let left_bar: String = "━".repeat(left);
    let right_bar: String = "━".repeat(right);
    // 使用 concat + 分段构造避免 format! 对 Unicode ━ 的计数问题
    let mut result = String::new();
    result.push_str(style::FG_CYAN);
    result.push_str(&left_bar);
    result.push_str(style::BOLD);
    result.push_str(&title_part);
    result.push_str(style::RESET);
    result.push_str(style::FG_CYAN);
    result.push_str(&right_bar);
    result.push_str(style::RESET);
    result
}

/// 轻量级分区（仅一行淡色文字）
pub fn dim_section(title: &str, icon: &str) -> String {
    format!(
        "{}{}{} {}{}",
        style::FG_BRIGHT_BLACK,
        icon,
        title,
        "─".repeat(40usize.saturating_sub(title.len() + 2)),
        style::RESET
    )
}

// =====================================================================
// 📊 表格渲染
// =====================================================================

/// 简单的两列表格行（用于 key-value 展示）
pub fn kv_row(key: &str, value: &str) -> String {
    format!(
        "  {}{:<20}{} {}",
        style::FG_BRIGHT_BLACK,
        key,
        style::RESET,
        value
    )
}

/// 带缩进的标签-值行
pub fn tag_value(tag: &str, value: &str, tag_width: usize) -> String {
    format!(
        "  {}{:<width$}{} {}",
        style::FG_BRIGHT_BLACK,
        tag,
        style::RESET,
        value,
        width = tag_width
    )
}

/// 三列表格头（用于 session list 等）
pub fn table_header(cols: &[&str], widths: &[usize]) -> String {
    let mut line = String::new();
    line.push_str(&format!("{}", style::FG_BRIGHT_BLACK));
    for (i, col) in cols.iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(15);
        line.push_str(&format!("  {:width$}", col, width = w));
    }
    line.push_str(&format!("{}", style::RESET));
    line
}

/// 三列表格行
pub fn table_row(cols: &[&str], widths: &[usize]) -> String {
    let mut line = String::new();
    for (i, col) in cols.iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(15);
        line.push_str(&format!("  {:width$}", col, width = w));
    }
    line
}

// =====================================================================
// 🌀 加载动画
// =====================================================================

/// 加载动画帧
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// 生成加载动画帧（带颜色）
pub fn spinner_frame(index: usize) -> String {
    let frame = SPINNER_FRAMES[index % SPINNER_FRAMES.len()];
    format!("{}{}{}", style::FG_CYAN, frame, style::RESET)
}

/// 生成加载状态行
pub fn loading_line(label: &str, frame_index: usize) -> String {
    format!(
        "{}{} {}{} {}...",
        style::CLEAR_LINE,
        style::FG_CYAN,
        SPINNER_FRAMES[frame_index % SPINNER_FRAMES.len()],
        style::RESET,
        label
    )
}

/// 🧠 思考文本（淡色显示）
pub fn thinking_text(content: &str) -> String {
    format!("{}{}{}", style::FG_BRIGHT_BLACK, content, style::RESET)
}

/// ⏳ 简单等待指示器
pub fn waiting_text(text: &str) -> String {
    format!(
        "{}{}⏳ {}{}",
        style::CLEAR_LINE,
        style::FG_YELLOW,
        text,
        style::RESET,
    )
}

/// ✅ 完成指示器（用于替换加载行）
pub fn done_text(text: &str) -> String {
    format!(
        "{}{}✅ {}{}",
        style::CLEAR_LINE,
        style::FG_GREEN,
        text,
        style::RESET,
    )
}

// =====================================================================
// 💬 提示符
// =====================================================================

/// 生成多彩提示符
/// mode: "user" (普通用户输入) | "auto" (自动模式)
pub fn prompt(mode: &str, extra: Option<&str>) -> String {
    let arrow = match mode {
        "auto" => format!("{}⟳{}", style::FG_MAGENTA, style::RESET),
        "tool" => format!("{}⚡{}", style::FG_YELLOW, style::RESET),
        _ => format!("{}❯{}", style::FG_CYAN, style::RESET),
    };

    let extra_part = match extra {
        Some(info) => format!(" {}│{} ", style::FG_BRIGHT_BLACK, info),
        None => String::new(),
    };

    format!(
        "\r{}{}{} {} ",
        style::CLEAR_LINE,
        extra_part,
        arrow,
        style::RESET
    )
}

/// 生成简短的上下文提示（用于提示符旁）
pub fn context_hint(token_usage: Option<f64>, msg_count: usize) -> String {
    match token_usage {
        Some(ratio) if ratio > 0.3 => {
            format!("{}💬{:.0}%{}", style::FG_BRIGHT_YELLOW, ratio * 100.0, style::RESET)
        }
        _ => {
            format!("{}💬{}{}", style::FG_BRIGHT_BLACK, msg_count, style::RESET)
        }
    }
}

/// 清除当前行并输出
pub fn clear_line() -> String {
    style::CLEAR_LINE.to_string()
}

/// 光标上移 n 行
pub fn cursor_up(n: usize) -> String {
    format!("\x1b[{}A", n)
}

// =====================================================================
// 🚀 启动横幅
// =====================================================================

/// 生成启动横幅
pub fn welcome_banner(version: &str, current_dir: &str, tool_count: usize) -> String {
    let mut output = String::new();
    
    // 顶部装饰
    output.push_str(&format!(
        "{}{}{}\n",
        style::FG_BRIGHT_BLACK,
        "╔══════════════════════════════════════════════════════════════╗",
        style::RESET
    ));
    
    // Logo + 标题行
    output.push_str(&format!(
        "{}║{}   {}🤖 {}{}{} {:>19}║\n",
        style::FG_BRIGHT_BLACK,
        style::RESET,
        style::BOLD,
        style::FG_CYAN,
        "Agent Lab",
        style::RESET,
        format!("{}v{}{}", style::FG_BRIGHT_BLACK, version, style::RESET),
    ));
    
    // 副标题
    output.push_str(&format!(
        "{}║{}   {}自我进化的 AI Agent 框架{}\n",
        style::FG_BRIGHT_BLACK,
        style::RESET,
        style::FG_BRIGHT_BLACK,
        style::RESET,
    ));
    
    // 分隔
    output.push_str(&format!(
        "{}║{}   {}──────────────────────────────────────────{}║\n",
        style::FG_BRIGHT_BLACK,
        style::RESET,
        style::DIM,
        style::RESET,
    ));
    
    // 信息行
    output.push_str(&format!(
        "{}║{}   {}📁 {}{:<42}{}║\n",
        style::FG_BRIGHT_BLACK,
        style::RESET,
        style::FG_BRIGHT_BLACK,
        style::RESET,
        truncate_str(current_dir, 42),
        style::FG_BRIGHT_BLACK,
    ));
    
    output.push_str(&format!(
        "{}║{}   {}🔧 {} 个工具已注册{}{:>24}║\n",
        style::FG_BRIGHT_BLACK,
        style::RESET,
        style::FG_BRIGHT_BLACK,
        tool_count,
        style::RESET,
        "",
    ));
    
    // 底部
    output.push_str(&format!(
        "{}║{}   {}💡 输入 /help 查看命令 {}{:>22}║\n",
        style::FG_BRIGHT_BLACK,
        style::RESET,
        style::FG_BRIGHT_BLACK,
        style::RESET,
        "",
    ));
    
    output.push_str(&format!(
        "{}{}{}\n",
        style::FG_BRIGHT_BLACK,
        "╚══════════════════════════════════════════════════════════════╝",
        style::RESET
    ));
    
    output
}

/// 简单版本 - 羽毛版欢迎
pub fn welcome_banner_compact(version: &str, current_dir: &str, tool_count: usize) -> String {
    format!(
        "{}━━━ {}🤖 {}{} v{} ━━━{}\n\
         {}📁 {}{}\n\
         {}🔧 {} 工具注册{} | 💡 输入 /help 查看命令\n",
        style::FG_CYAN,
        style::BOLD,
        "Agent Lab",
        style::RESET,
        version,
        style::RESET,
        style::FG_BRIGHT_BLACK,
        style::RESET,
        current_dir,
        style::FG_BRIGHT_BLACK,
        tool_count,
        style::RESET,
    )
}

// =====================================================================
// 🧰 工具函数
// =====================================================================

/// 截断字符串到指定长度，追加 "..."
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// 格式化持续时间（秒 → 人类可读）
pub fn format_duration(secs: f64) -> String {
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else if secs < 60.0 {
        format!("{:.1}s", secs)
    } else {
        let mins = (secs / 60.0) as u64;
        let remaining_secs = (secs % 60.0) as u64;
        format!("{}m {}s", mins, remaining_secs)
    }
}

/// 格式化会话时间（ISO 时间 → 简洁格式）
pub fn format_session_time(iso_time: &str) -> String {
    // 简单截取到分钟
    if iso_time.len() >= 19 {
        let date = &iso_time[..10];
        let time = &iso_time[11..19];
        format!("{} {}", date, time)
    } else {
        iso_time.to_string()
    }
}

/// 生成进度条
pub fn progress_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    
    let bar_color = if ratio > 0.85 {
        style::FG_RED
    } else if ratio > 0.65 {
        style::FG_YELLOW
    } else {
        style::FG_GREEN
    };
    
    format!(
        "{}▰{}{}{}▱{}",
        bar_color,
        "▰".repeat(filled),
        style::FG_BRIGHT_BLACK,
        "▱".repeat(empty),
        style::RESET
    )
}

// =====================================================================
// ⏱️ 简单的执行计时器
// =====================================================================

pub struct Timer {
    start: Instant,
}

impl Timer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    pub fn elapsed_str(&self) -> String {
        format_duration(self.elapsed())
    }

    pub fn reset(&mut self) {
        self.start = Instant::now();
    }
}
