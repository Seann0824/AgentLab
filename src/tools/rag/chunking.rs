use scirs2_text::is_cjk_char;

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct Paragraph {
    pub content: String,
    pub heading_path: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// 按 Markdown 标题层级把文本拆分成段落，保留标题路径信息。
pub fn split_paragraphs_with_headings(text: String) -> Vec<Paragraph> {
    // 使用 split_inclusive 保留换行符信息，让 char_pos 能精确对应原文位置
    let lines = text.split_inclusive('\n');
    let mut heading_stack: Vec<String> = vec![];
    let mut paragraphs: Vec<Paragraph> = vec![];

    let mut buf: Vec<String> = vec![];
    let mut char_pos: usize = 0;
    let mut paragraph_start: usize = 0;

    let flush_buf = |end_pos: usize,
                     heading_stack: &[String],
                     buf: &[String],
                     start_pos: usize,
                     paragraphs: &mut Vec<Paragraph>| {
        if buf.is_empty() {
            return;
        }

        let content = buf.join("\n").trim().to_string();
        if content.is_empty() {
            return;
        }
        let heading_path =
            (!heading_stack.is_empty()).then(|| heading_stack.join(" > ").trim().to_string());

        paragraphs.push(Paragraph {
            start: start_pos,
            end: end_pos,
            content,
            heading_path,
        })
    };

    for line_with_sep in lines {
        // 去掉行尾的换行符（兼容 \r\n 和 \n）
        let raw = line_with_sep
            .strip_suffix('\n')
            .map(|s| s.strip_suffix('\r').unwrap_or(s))
            .unwrap_or(line_with_sep);

        if raw.trim().starts_with("#") {
            flush_buf(
                char_pos,
                &heading_stack,
                &buf,
                paragraph_start,
                &mut paragraphs,
            );
            buf.clear();

            let mut level = raw.len() - raw.trim_start_matches("#").len();
            let title = raw.trim_start_matches("#").trim().to_string();

            if level <= 0 {
                level = 1;
            }

            // 层级小了说明前面的文本内容都处理完成了，把处理完的标题弹出
            while level <= heading_stack.len() {
                heading_stack.pop();
            }
            heading_stack.push(title);

            char_pos += line_with_sep.len();
            continue;
        }
        // 段落内容积累
        if raw.trim().is_empty() {
            flush_buf(
                char_pos,
                &heading_stack,
                &buf,
                paragraph_start,
                &mut paragraphs,
            );
            buf.clear();
        } else {
            if buf.is_empty() {
                paragraph_start = char_pos;
            }
            buf.push(raw.to_string());
        }
        char_pos += line_with_sep.len();
    }

    flush_buf(
        char_pos,
        &heading_stack,
        &buf,
        paragraph_start,
        &mut paragraphs,
    );

    if paragraphs.is_empty() {
        paragraphs.push(Paragraph {
            start: 0,
            end: text.len(),
            content: text,
            heading_path: None,
        });
    }

    paragraphs
}

/// 在结构化段落划分的基础上，根据 Token 数量进行智能分块。
///
/// 注意：overlap 部分会出现在相邻 chunk 中，这是为了保证检索时上下文的连续性，
/// 属于 RAG 中常见的冗余设计。如果不需要重叠，可把 overlap_tokens 设为 0。
pub fn chunk_paragraphs(
    paragraphs: Vec<Paragraph>,
    chunk_tokens: usize,
    overlap_tokens: usize,
) -> Vec<Paragraph> {
    let mut chunks: Vec<Paragraph> = vec![];
    let mut current_chunk: Vec<Paragraph> = vec![];
    let mut current_tokens = 0usize;

    let build_chunk = |current_chunk: &Vec<Paragraph>| {
        let start = current_chunk
            .first()
            .and_then(|p| Some(p.start))
            .unwrap_or(0usize);
        let end = current_chunk
            .last()
            .and_then(|p| Some(p.end))
            .unwrap_or(0usize);
        let heading_path = current_chunk
            .iter()
            .rev()
            .filter_map(|x| x.heading_path.as_ref())
            .find(|s| !s.is_empty())
            .cloned();
        let content = current_chunk
            .iter()
            .map(|p| p.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        Paragraph {
            start,
            end,
            heading_path,
            content,
        }
    };

    for paragraph in paragraphs {
        let paragraph_tokens = approx_token_len(&paragraph.content);

        if paragraph_tokens + current_tokens <= chunk_tokens || current_chunk.is_empty() {
            current_chunk.push(paragraph);
            current_tokens += paragraph_tokens;
        } else {
            // 处理当前 chunk
            chunks.push(build_chunk(&current_chunk));

            // 构建重叠部分保证语义连通性，作为下一个 chunk 的开头
            if overlap_tokens > 0 && !current_chunk.is_empty() {
                let mut next_chunk_start: Vec<Paragraph> = vec![];
                let mut start_tokens: usize = 0;

                for p in current_chunk.iter().rev() {
                    let p_tokens = approx_token_len(&p.content);
                    if p_tokens + start_tokens > overlap_tokens {
                        break;
                    }

                    next_chunk_start.push(p.clone());
                    start_tokens += p_tokens;
                }

                // 恢复原文顺序
                next_chunk_start.reverse();
                current_chunk = next_chunk_start;
                current_tokens = start_tokens;
            } else {
                current_chunk.clear();
                current_tokens = 0;
            }

            // 把当前段落加入新的 chunk
            current_chunk.push(paragraph);
            current_tokens += paragraph_tokens;
        }
    }

    // 处理最后一个块
    if !current_chunk.is_empty() {
        chunks.push(build_chunk(&current_chunk));
    }

    chunks
}

/// 基于 CJK 字符的简易 token 长度估算。
pub fn approx_token_len(content: &str) -> usize {
    content
        .split_whitespace()
        .map(|token| {
            let mut cjk_count = 0usize;
            let mut non_cjk_count = 0usize;
            for ch in token.chars() {
                if is_cjk_char(ch) {
                    cjk_count += 1;
                } else {
                    non_cjk_count += 1;
                }
            }
            // CJK 字符每个算 1 个 token；非 CJK 的整个 token 算 1 个
            cjk_count + if non_cjk_count > 0 { 1 } else { 0 }
        })
        .sum()
}
