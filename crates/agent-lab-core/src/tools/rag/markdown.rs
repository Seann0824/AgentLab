use regex::Regex;
use std::sync::OnceLock;

/// 缓存所有 markdown 清洗正则，避免每次调用重新编译。
struct MarkdownRegexes {
    headers: Regex,
    links: Regex,
    reference_links: Regex,
    images: Regex,
    code_blocks: Regex,
    bold_asterisks: Regex,
    bold_underscores: Regex,
    italic_asterisks: Regex,
    italic_underscores: Regex,
    strikethrough: Regex,
    inline_code: Regex,
    html_tags: Regex,
    blockquotes: Regex,
    blank_lines: Regex,
    spaces: Regex,
}

impl MarkdownRegexes {
    fn get() -> &'static MarkdownRegexes {
        static INSTANCE: OnceLock<MarkdownRegexes> = OnceLock::new();
        INSTANCE.get_or_init(|| MarkdownRegexes {
            headers: Regex::new(r"(?m)^#{1,6}\s+").unwrap(),
            links: Regex::new(r"\[([^\]]+)\]\([^)]+?\)").unwrap(),
            reference_links: Regex::new(r"\[([^\]]+)\]\[[^\]]*\]").unwrap(),
            images: Regex::new(r"!\[([^\]]*)\]\([^)]+?\)").unwrap(),
            code_blocks: Regex::new(r"```[^\n]*\n([\s\S]*?)```").unwrap(),
            bold_asterisks: Regex::new(r"\*\*([^*]+?)\*\*").unwrap(),
            bold_underscores: Regex::new(r"__([^_]+?)__").unwrap(),
            italic_asterisks: Regex::new(r"\*([^*]+?)\*").unwrap(),
            italic_underscores: Regex::new(r"_([^_]+?)_").unwrap(),
            strikethrough: Regex::new(r"~~([^~]+?)~~").unwrap(),
            inline_code: Regex::new(r"`([^`]+)`").unwrap(),
            html_tags: Regex::new(r"<[^>]+>").unwrap(),
            blockquotes: Regex::new(r"(?m)^>\s?").unwrap(),
            blank_lines: Regex::new(r"\n\s*\n").unwrap(),
            spaces: Regex::new(r"[ \t]+").unwrap(),
        })
    }
}

/// 预处理 Markdown 文本，去掉标记符号但保留语义内容，用于生成更干净的 embedding。
pub fn preprocess_markdown_for_embedding(content: &str) -> String {
    let re = MarkdownRegexes::get();

    // 1. 代码块（必须先处理，否则 inline code 会误吃 ``` 里的反引号）
    let text = re.code_blocks.replace_all(content, "$1");

    // 2. 行内代码
    let text = re.inline_code.replace_all(&text, "$1");

    // 3. 图片与链接：保留可见文本/alt 文本
    // 必须先处理图片，否则普通链接正则会吃掉 ![alt](url) 里的 [alt](url)
    let text = re.images.replace_all(&text, "$1");
    let text = re.links.replace_all(&text, "$1");
    let text = re.reference_links.replace_all(&text, "$1");

    // 4. 强调：先粗体（双标记），再斜体（单标记），避免 `_text_` 吃掉 `__text__`
    let text = re.bold_asterisks.replace_all(&text, "$1");
    let text = re.bold_underscores.replace_all(&text, "$1");
    let text = re.italic_asterisks.replace_all(&text, "$1");
    let text = re.italic_underscores.replace_all(&text, "$1");
    let text = re.strikethrough.replace_all(&text, "$1");

    // 5. 标题符号
    let text = re.headers.replace_all(&text, "");

    // 6. HTML 标签与 blockquote 标记
    let text = re.html_tags.replace_all(&text, " ");
    let text = re.blockquotes.replace_all(&text, "");

    // 7. 空白规范化
    let text = re.blank_lines.replace_all(&text, "\n\n");
    let text = re.spaces.replace_all(&text, " ");

    text.trim().to_string()
}
