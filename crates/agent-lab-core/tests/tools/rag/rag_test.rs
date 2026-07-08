use agent_lab_core::tools::rag::{Paragraph, RagTool};

fn tool() -> RagTool {
    RagTool::new()
}

#[test]
fn test_approx_token_len_empty() {
    assert_eq!(tool().approx_token_len(""), 0);
}

#[test]
fn test_approx_token_len_english_words() {
    let t = tool();
    assert_eq!(t.approx_token_len("hello world"), 2);
    assert_eq!(t.approx_token_len("  foo   bar  baz  "), 3);
}

#[test]
fn test_approx_token_len_cjk() {
    let t = tool();
    assert_eq!(t.approx_token_len("你好世界"), 4);
    assert_eq!(t.approx_token_len("这是一段中文文本"), 8);
}

#[test]
fn test_approx_token_len_mixed() {
    let t = tool();
    // "hello" 算 1 个，"你好" 算 2 个，"world" 算 1 个
    assert_eq!(t.approx_token_len("hello 你好 world"), 4);
}

#[test]
fn test_split_no_headings() {
    let text = "first line\nsecond line".to_string();
    let ps = tool().split_paragraphs_with_headings(text.clone());

    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].content, "first line\nsecond line");
    assert_eq!(ps[0].heading_path, None);
    assert_eq!(ps[0].start, 0);
    assert_eq!(ps[0].end, text.len());
}

#[test]
fn test_split_empty_lines() {
    let text = "para one line one\npara one line two\n\npara two line one".to_string();
    let ps = tool().split_paragraphs_with_headings(text);

    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].content, "para one line one\npara one line two");
    assert_eq!(ps[1].content, "para two line one");
}

#[test]
fn test_split_single_heading() {
    let text = "# Title\n\nbody line one\nbody line two".to_string();
    let ps = tool().split_paragraphs_with_headings(text);

    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].content, "body line one\nbody line two");
    assert_eq!(ps[0].heading_path, Some("Title".to_string()));
}

#[test]
fn test_split_heading_hierarchy() {
    let text = "# Chapter 1\n\nintro text\n\n## Section 1.1\n\nsection body\n\n# Chapter 2\n\nchapter two body".to_string();
    let ps = tool().split_paragraphs_with_headings(text);

    assert_eq!(ps.len(), 3);
    assert_eq!(ps[0].heading_path, Some("Chapter 1".to_string()));
    assert_eq!(ps[1].heading_path, Some("Chapter 1 > Section 1.1".to_string()));
    assert_eq!(ps[2].heading_path, Some("Chapter 2".to_string()));
}

#[test]
fn test_split_heading_resets_stack() {
    let text = "# A\n\n## A1\n\nbody one\n\n# B\n\nbody two".to_string();
    let ps = tool().split_paragraphs_with_headings(text);

    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].heading_path, Some("A > A1".to_string()));
    assert_eq!(ps[1].heading_path, Some("B".to_string()));
}

#[test]
fn test_split_buf_cleared_after_heading() {
    // 标题后面的段落不应该包含标题前面的内容
    let text = "before heading\n\n# Heading\n\nafter heading".to_string();
    let ps = tool().split_paragraphs_with_headings(text);

    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].content, "before heading");
    assert_eq!(ps[1].content, "after heading");
}

#[test]
fn test_chunk_basic() {
    let rag = tool();
    let paragraphs = vec![
        Paragraph {
            content: "hello world".to_string(),
            heading_path: None,
            start: 0,
            end: 11,
        },
        Paragraph {
            content: "foo bar baz".to_string(),
            heading_path: None,
            start: 12,
            end: 23,
        },
    ];

    // chunk_tokens 足够大，全部放进一个 chunk
    let chunks = rag.chunk_paragraphs(paragraphs.clone(), 100, 0);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content, "hello world\n\nfoo bar baz");
    assert_eq!(chunks[0].start, 0);
    assert_eq!(chunks[0].end, 23);
}

#[test]
fn test_chunk_split() {
    let rag = tool();
    let paragraphs = vec![
        Paragraph {
            content: "hello world".to_string(),
            heading_path: Some("A".to_string()),
            start: 0,
            end: 11,
        },
        Paragraph {
            content: "foo bar baz".to_string(),
            heading_path: Some("B".to_string()),
            start: 12,
            end: 23,
        },
    ];

    // 每个 paragraph 2 token，chunk_tokens=2 刚好放一个
    let chunks = rag.chunk_paragraphs(paragraphs, 2, 0);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].content, "hello world");
    assert_eq!(chunks[0].heading_path, Some("A".to_string()));
    assert_eq!(chunks[1].content, "foo bar baz");
    assert_eq!(chunks[1].heading_path, Some("B".to_string()));
}

#[test]
fn test_chunk_overlap() {
    let rag = tool();
    let paragraphs = vec![
        Paragraph {
            content: "one two".to_string(),
            heading_path: None,
            start: 0,
            end: 7,
        },
        Paragraph {
            content: "three four".to_string(),
            heading_path: None,
            start: 8,
            end: 18,
        },
        Paragraph {
            content: "five six".to_string(),
            heading_path: None,
            start: 19,
            end: 27,
        },
    ];

    // 每个 paragraph 2 token，chunk_tokens=2，overlap_tokens=2
    // chunk1: [one two]
    // chunk2: [one two] + overlap + [three four] ？ 不，算法会把当前 paragraph 加进新 chunk
    let chunks = rag.chunk_paragraphs(paragraphs, 2, 2);

    // 至少应该有 3 个 chunk（每个原始 paragraph 一个）
    assert_eq!(chunks.len(), 3);
    assert!(chunks[0].content.contains("one two"));
    assert!(chunks[1].content.contains("three four"));
    assert!(chunks[2].content.contains("five six"));
}

#[test]
fn test_chunk_does_not_drop_paragraph() {
    // 验证修复：触发 chunk 切换时，当前段落不会丢失
    let rag = tool();
    let paragraphs = vec![
        Paragraph {
            content: "aaa bbb".to_string(),
            heading_path: None,
            start: 0,
            end: 7,
        },
        Paragraph {
            content: "ccc ddd".to_string(),
            heading_path: None,
            start: 8,
            end: 15,
        },
    ];

    let chunks = rag.chunk_paragraphs(paragraphs, 2, 0);

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].content, "aaa bbb");
    assert_eq!(chunks[1].content, "ccc ddd");
}

#[test]
fn test_preprocess_headers() {
    let rag = tool();
    let input = "# Title\n## Section\n### Sub\nbody text";
    assert_eq!(
        rag.preprocess_markdown_for_embedding(input),
        "Title\nSection\nSub\nbody text"
    );
}

#[test]
fn test_preprocess_links() {
    let rag = tool();
    let input = "See [example](https://example.com) for details";
    assert_eq!(
        rag.preprocess_markdown_for_embedding(input),
        "See example for details"
    );
}

#[test]
fn test_preprocess_emphasis() {
    let rag = tool();
    let input = "**bold** and *italic* text";
    assert_eq!(
        rag.preprocess_markdown_for_embedding(input),
        "bold and italic text"
    );
}

#[test]
fn test_preprocess_inline_code() {
    let rag = tool();
    let input = "Use `cargo build` to compile";
    assert_eq!(
        rag.preprocess_markdown_for_embedding(input),
        "Use cargo build to compile"
    );
}

#[test]
fn test_preprocess_code_block() {
    let rag = tool();
    let input = "```rust\nlet x = 1;\nlet y = 2;\n```";
    assert_eq!(
        rag.preprocess_markdown_for_embedding(input),
        "let x = 1;\nlet y = 2;"
    );
}

#[test]
fn test_preprocess_excessive_whitespace() {
    let rag = tool();
    let input = "line one\n\n\n\n   \nline two\t\tthree";
    assert_eq!(
        rag.preprocess_markdown_for_embedding(input),
        "line one\n\nline two three"
    );
}

#[test]
fn test_preprocess_combined() {
    let rag = tool();
    let input = "# Intro\n\nRead [docs](https://docs.rs) and run `cargo test`.\n\n**Note**: *important* code below.\n\n```rust\nfn main() {}\n```";
    assert_eq!(
        rag.preprocess_markdown_for_embedding(input),
        "Intro\n\nRead docs and run cargo test.\n\nNote: important code below.\n\nfn main() {}"
    );
}

#[test]
fn test_preprocess_bold_underscore() {
    let rag = tool();
    assert_eq!(
        rag.preprocess_markdown_for_embedding("__bold__ text"),
        "bold text"
    );
}

#[test]
fn test_preprocess_italic_underscore() {
    let rag = tool();
    assert_eq!(
        rag.preprocess_markdown_for_embedding("_italic_ text"),
        "italic text"
    );
}

#[test]
fn test_preprocess_bold_underscore_not_eaten_by_italic() {
    let rag = tool();
    // __text__ 应该整体变成 text，而不是被 _text_ 截断
    assert_eq!(
        rag.preprocess_markdown_for_embedding("__text__"),
        "text"
    );
}

#[test]
fn test_preprocess_strikethrough() {
    let rag = tool();
    assert_eq!(
        rag.preprocess_markdown_for_embedding("~~deleted~~ text"),
        "deleted text"
    );
}

#[test]
fn test_preprocess_images() {
    let rag = tool();
    assert_eq!(
        rag.preprocess_markdown_for_embedding("![diagram](https://example.com/d.png)"),
        "diagram"
    );
}

#[test]
fn test_preprocess_reference_links() {
    let rag = tool();
    assert_eq!(
        rag.preprocess_markdown_for_embedding("See [docs][ref] for more."),
        "See docs for more."
    );
}

#[test]
fn test_preprocess_html_tags() {
    let rag = tool();
    assert_eq!(
        rag.preprocess_markdown_for_embedding("Hello <br> world"),
        "Hello world"
    );
}

#[test]
fn test_preprocess_blockquotes() {
    let rag = tool();
    assert_eq!(
        rag.preprocess_markdown_for_embedding("> quoted text\n> more quote"),
        "quoted text\nmore quote"
    );
}

#[test]
fn test_preprocess_emphasis_order() {
    let rag = tool();
    // 混合强调：粗体里包含斜体的情况
    assert_eq!(
        rag.preprocess_markdown_for_embedding("**bold and _italic_**"),
        "bold and italic"
    );
}

use agent_lab_core::tools::rag::RagIndex;
use sqlx::PgPool;

#[tokio::test]
async fn test_rag_index_empty_chunks() {
    dotenvy::dotenv().ok();
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        // 没有数据库环境时跳过
        return;
    };
    let Ok(db) = PgPool::connect(&database_url).await else {
        return;
    };

    let index = RagIndex::with_default_embedder(db);
    let result = index.index_chunks(vec![], "test.md", "default", 64).await;
    assert!(result.is_ok());
}
