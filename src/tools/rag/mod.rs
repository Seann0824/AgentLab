use std::{collections::HashMap, thread::panicking};

use scirs2_text::is_cjk_char;

use crate::tools::types::Tool;

struct RagTool {}

struct Paragraph {
    content: String,
    heading_path: Option<String>,
    start: usize,
    end: usize,
}

impl RagTool {
    // 获取 markdown 内容
    fn get_markdown_content(&self, path: &str) -> String {
        "".to_string()
    }

    fn split_paragraphs_with_headings(&self, text: String) -> Vec<Paragraph> {
        let lines = text.lines();
        let mut heading_stack: Vec<String> = vec![];
        let mut paragraphs: Vec<Paragraph> = vec![];

        let mut buf: Vec<String> = vec![];
        let mut char_pos: usize = 0;

        // TODO: 感觉这里的start和end计算有问题，因为我们剔除了空白。
        let mut flush_buf = |end_pos: usize, heading_stack: &Vec<String>, buf: &Vec<String>| {
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
                start: 0usize.max(end_pos - content.len()),
                end: end_pos,
                content,
                heading_path: heading_path,
            })
        };

        for line in lines {
            let raw = line;
            if raw.trim().starts_with("#") {
                flush_buf(char_pos, &heading_stack, &buf);
                let mut level = raw.len() - raw.trim_start_matches("#").len();
                let title = raw.trim_start_matches("#").trim().to_string();

                if level <= 0 {
                    level = 1;
                }

                // 层级小了说明前面的文本内容都处理完成了， 把处理完的标题弹出
                while level <= heading_stack.len() {
                    heading_stack.pop();
                }
                heading_stack.push(title);

                char_pos += raw.len() + 1;
                continue;
            }
            // 段落内容积累
            if raw.trim().is_empty() {
                flush_buf(char_pos, &heading_stack, &buf);
                buf.clear();
            } else {
                buf.push(raw.to_string());
            }
            char_pos += raw.len() + 1;
        }

        flush_buf(char_pos, &heading_stack, &buf);

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

    // 在结构化段落划分的基础上，根据 Token 数量进行智能分块
    fn chunk_paragraphs(
        &self,
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
            let paragraph_tokens = self.approx_token_len(&paragraph.content);

            if paragraph_tokens + current_tokens <= chunk_tokens || current_chunk.is_empty() {
                current_chunk.push(paragraph);
                current_tokens += paragraph_tokens;
            } else {
                // 处理当前 chunk
                chunks.push(build_chunk(&current_chunk));

                // 构建重叠部分保证语义连通性, 作为下一个chunk的开头，但是我感觉这个会冗余，后续我们要判断一下
                if overlap_tokens > 0 && !current_chunk.is_empty() {
                    let mut next_chunk_start: Vec<Paragraph> = vec![];
                    let mut start_tokens: usize = 0;

                    for p in current_chunk.into_iter().rev() {
                        let paragraph_tokens = self.approx_token_len(&p.content);
                        if paragraph_tokens + start_tokens > overlap_tokens {
                            break;
                        }

                        next_chunk_start.push(p);
                        start_tokens += paragraph_tokens;
                    }

                    current_chunk = next_chunk_start;
                    current_tokens = start_tokens;
                } else {
                    current_chunk.clear();
                    current_tokens = 0;
                }
            }
        }

        // 处理最后一个块， 这是是否要处理？因为里面会有我们 overlap 的部分，可能导致冗余
        if !current_chunk.is_empty() {
            chunks.push(build_chunk(&current_chunk));
        }

        chunks
    }

    fn approx_token_len(&self, content: &str) -> usize {
        let is_cjk = |ch: char| {
            matches!(ch as u32,
                0x4E00..=0x9FFF |   // CJK统一汉字
                0x3400..=0x4DBF |   // CJK扩展A
                0x20000..=0x2A6DF | // CJK扩展B
                0x2A700..=0x2B73F | // CJK扩展C
                0x2B740..=0x2B81F | // CJK扩展D
                0x2B820..=0x2CEAF | // CJK扩展E
                0xF900..=0xFAFF     // CJK兼容汉字
            )
        };

        let cjk = content.chars().filter(|&ch| is_cjk(ch)).count();
        let non_cjk_tokens = content.split_whitespace().filter(|t| !t.is_empty()).count();
        cjk + non_cjk_tokens
    }
}

#[async_trait::async_trait]
impl Tool for RagTool {
    fn name(&self) -> &str {
        todo!()
    }

    fn description(&self) -> &str {
        todo!()
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        todo!()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        todo!()
    }
}
