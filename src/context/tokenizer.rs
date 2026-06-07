use std::sync::OnceLock;

use crate::model::ChatMessage;

/// ⭐ 全局共享的 Tokenizer 实例（懒加载，优雅降级）
///
/// 如果 tiktoken 初始化失败（如 WASM 环境、缺失数据文件），
/// 回退到轻量级字符统计估算（精度 ±20%）。
fn global_tokenizer() -> Option<&'static tiktoken_rs::CoreBPE> {
    static TOKENIZER: OnceLock<Option<tiktoken_rs::CoreBPE>> = OnceLock::new();
    TOKENIZER
        .get_or_init(|| {
            match tiktoken_rs::cl100k_base() {
                Ok(bpe) => {
                    eprintln!("[TokenEstimator] tiktoken cl100k_base initialized successfully");
                    Some(bpe)
                }
                Err(e) => {
                    eprintln!(
                        "[TokenEstimator] WARNING: tiktoken init failed ({}), falling back to char-count estimation",
                        e
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// ⭐ Token 估算器
///
/// 使用 tiktoken-rs (cl100k_base) 进行精确 token 计数。
/// 如果 tiktoken 不可用，自动降级为轻量级字符统计估算。
///
/// 缓存优化：增量式更新 —— 每次只计算新消息的 token 数，
/// 缓存总计数避免全量遍历 O(n)。
#[derive(Debug, Clone)]
pub struct TokenEstimator {
    /// 校准系数（模型实际 tokenizer 与 cl100k_base 的比率）
    calibration_factor: f64,
    /// 已校准标记
    calibrated: bool,
    /// ⭐ 是否使用精确 tokenizer（否则使用降级估算）
    use_precise: bool,
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenEstimator {
    pub fn new() -> Self {
        let use_precise = global_tokenizer().is_some();
        TokenEstimator {
            calibration_factor: 1.0,
            calibrated: false,
            use_precise,
        }
    }

    /// ⭐ 估算文本的 token 数
    ///
    /// 优先使用 tiktoken 精确计算，降级时使用字符统计经验公式。
    pub fn estimate_text(&self, text: &str) -> usize {
        let count = if self.use_precise {
            // 精确模式：使用 tiktoken
            if let Some(bpe) = global_tokenizer() {
                let tokens = bpe.encode_with_special_tokens(text);
                tokens.len()
            } else {
                // 降级模式：使用字符统计
                self.estimate_text_fallback(text)
            }
        } else {
            self.estimate_text_fallback(text)
        };

        // 应用校准系数
        ((count as f64) * self.calibration_factor).ceil() as usize
    }

    /// ⭐ 轻量级降级估算（字符统计经验公式）
    ///
    /// 估算规则：
    /// - 英文: ~4 chars/token
    /// - 中文: ~1.5 chars/token
    /// - 代码: ~3 chars/token
    /// - 特殊字符: ~0.5 token/char
    ///
    /// 精度目标: ±20%
    fn estimate_text_fallback(&self, text: &str) -> usize {
        let mut tokens = 0f64;
        let mut ascii_count = 0usize;
        let mut cjk_count = 0usize;

        for ch in text.chars() {
            match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' => ascii_count += 1,
                '\u{4e00}'..='\u{9fff}' | '\u{3000}'..='\u{303f}' => cjk_count += 1,
                ' ' | '\n' | '\t' => tokens += 0.25,
                _ => tokens += 0.5,
            }
        }

        tokens += ascii_count as f64 / 4.0;
        tokens += cjk_count as f64 / 1.5;

        (tokens.ceil() as usize).max(1)
    }

    /// ⭐ 估算单条消息的 token 数（增量更新用，O(1)）
    ///
    /// 缓存优化核心：每次只计算新添加的消息，不遍历全量历史。
    pub fn estimate_message(&self, msg: &ChatMessage) -> usize {
        // ChatML 格式的消息结构开销（参考 OpenAI 官方 tokenizer 行为）
        let overhead = match msg {
            ChatMessage::System { .. } => 4,  // <|im_start|>system\n ... <|im_end|>\n
            ChatMessage::User { .. } => 3,    // <|im_start|>user\n ... <|im_end|>
            ChatMessage::Assistant { .. } => 3,
            ChatMessage::Tool { .. } => 3,
        };

        let text_tokens = match msg {
            ChatMessage::System { content }
            | ChatMessage::User { content }
            | ChatMessage::Tool { content, .. } => self.estimate_text(content),
            ChatMessage::Assistant {
                content,
                tool_calls,
            } => {
                let content_tokens = self.estimate_text(content);
                let tool_calls_tokens: usize = tool_calls
                    .iter()
                    .map(|tc| {
                        self.estimate_text(&tc.id)
                            + self.estimate_text(&tc.name)
                            + self.estimate_text(&tc.arguments)
                    })
                    .sum();
                content_tokens + tool_calls_tokens
            }
        };

        text_tokens + overhead
    }

    /// 全量估算消息列表的 token 总数（用于缓存失效后的重新计算）
    /// 复杂度 O(n)，仅在缓存失效时调用
    pub fn estimate_messages(&self, messages: &[ChatMessage]) -> usize {
        messages
            .iter()
            .map(|msg| self.estimate_message(msg))
            .sum()
    }

    /// 校准：用实际 token 计数调整校准系数
    pub fn calibrate(&mut self, estimated: usize, actual: usize) {
        if estimated > 0 && actual > 0 {
            self.calibration_factor = actual as f64 / estimated as f64;
            self.calibrated = true;
        }
    }

    /// 是否已校准
    pub fn is_calibrated(&self) -> bool {
        self.calibrated
    }

    /// 获取校准系数
    pub fn calibration_factor(&self) -> f64 {
        self.calibration_factor
    }

    /// 是否使用精确 tokenizer
    pub fn use_precise(&self) -> bool {
        self.use_precise
    }

    /// 格式化 token 数为可读字符串（如 "23.5K"）
    pub fn format_tokens(count: usize) -> String {
        if count >= 1_000_000 {
            format!("{:.1}M", count as f64 / 1_000_000.0)
        } else if count >= 1_000 {
            format!("{:.1}K", count as f64 / 1_000.0)
        } else {
            count.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_short_text() {
        let estimator = TokenEstimator::new();
        let tokens = estimator.estimate_text("Hello, world!");
        assert!(
            tokens > 0 && tokens < 10,
            "Short text should be a few tokens, got {}",
            tokens
        );
    }

    #[test]
    fn test_estimate_cjk_text() {
        let estimator = TokenEstimator::new();
        let tokens = estimator.estimate_text("你好，世界！");
        assert!(tokens > 0, "CJK text should have tokens, got {}", tokens);
    }

    #[test]
    fn test_estimate_code_text() {
        let estimator = TokenEstimator::new();
        let code = "fn main() {\n    println!(\"Hello\");\n}";
        let tokens = estimator.estimate_text(code);
        assert!(
            tokens > 5,
            "Code should be more than 5 tokens, got {}",
            tokens
        );
    }

    #[test]
    fn test_fallback_estimator() {
        let estimator = TokenEstimator::new();
        // 强制使用降级估算
        let tokens = estimator.estimate_text_fallback("Hello, world! This is a test.");
        assert!(
            tokens > 0 && tokens < 20,
            "Fallback should give reasonable estimate, got {}",
            tokens
        );
    }

    #[test]
    fn test_estimate_message_types() {
        let estimator = TokenEstimator::new();

        let sys = ChatMessage::system("You are a helpful assistant.");
        let user = ChatMessage::user("Hello!");
        let assistant = ChatMessage::assistant("Hi there!");
        let tool = ChatMessage::tool("call_123", r#"{"ok": true}"#);

        assert!(estimator.estimate_message(&sys) > 0);
        assert!(estimator.estimate_message(&user) > 0);
        assert!(estimator.estimate_message(&assistant) > 0);
        assert!(estimator.estimate_message(&tool) > 0);
    }

    #[test]
    fn test_estimate_messages_list() {
        let estimator = TokenEstimator::new();
        let messages = vec![
            ChatMessage::system("You are helpful."),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi!"),
        ];
        let total = estimator.estimate_messages(&messages);
        assert!(total > 0);
    }

    #[test]
    fn test_calibrate() {
        let mut estimator = TokenEstimator::new();
        assert!(!estimator.is_calibrated());
        estimator.calibrate(100, 120);
        assert!(estimator.is_calibrated());
        assert!((estimator.calibration_factor() - 1.2).abs() < 1e-6);
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(TokenEstimator::format_tokens(500), "500");
        assert_eq!(TokenEstimator::format_tokens(1500), "1.5K");
        assert_eq!(TokenEstimator::format_tokens(2_000_000), "2.0M");
    }

    #[test]
    fn test_estimate_consistent() {
        let estimator = TokenEstimator::new();
        // 验证相同文本得到相同结果
        let a = estimator.estimate_text("consistent test");
        let b = estimator.estimate_text("consistent test");
        assert_eq!(a, b);
    }
}
