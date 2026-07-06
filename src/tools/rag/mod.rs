use std::collections::HashMap;

use scirs2_text::is_cjk_char;

use crate::tools::types::Tool;

pub struct RagTool {}

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct Paragraph {
    pub content: String,
    pub heading_path: Option<String>,
    pub start: usize,
    pub end: usize,
}

impl RagTool {
    pub fn new() -> Self {
        Self {}
    }

    // 获取 markdown 内容
    pub fn get_markdown_content(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {}: {}", path, e))
    }

    pub fn split_paragraphs_with_headings(&self, text: String) -> Vec<Paragraph> {
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

    // 在结构化段落划分的基础上，根据 Token 数量进行智能分块。
    // 注意：overlap 部分会出现在相邻 chunk 中，这是为了保证检索时上下文的连续性，
    // 属于 RAG 中常见的冗余设计。如果不需要重叠，可把 overlap_tokens 设为 0。
    pub fn chunk_paragraphs(
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

                // 构建重叠部分保证语义连通性，作为下一个 chunk 的开头
                if overlap_tokens > 0 && !current_chunk.is_empty() {
                    let mut next_chunk_start: Vec<Paragraph> = vec![];
                    let mut start_tokens: usize = 0;

                    for p in current_chunk.iter().rev() {
                        let p_tokens = self.approx_token_len(&p.content);
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

    pub fn approx_token_len(&self, content: &str) -> usize {
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

    
}

#[async_trait::async_trait]
impl Tool for RagTool {
    fn name(&self) -> &str {
        "rag"
    }

    fn description(&self) -> &str {
        "读取本地 Markdown 文件，按标题结构分割段落并做 token 分块，返回可用于检索的文本块列表。"
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let mut properties = HashMap::new();
        properties.insert(
            "path".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("要读取的 Markdown 文件路径".to_string()),
                ..Default::default()
            }),
        );
        properties.insert(
            "chunk_tokens".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::Number),
                description: Some("每个 chunk 的最大 token 数，默认 1024".to_string()),
                ..Default::default()
            }),
        );
        properties.insert(
            "overlap_tokens".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::Number),
                description: Some("相邻 chunk 之间的重叠 token 数，默认 128".to_string()),
                ..Default::default()
            }),
        );
        openai_api_rs::v1::types::FunctionParameters {
            schema_type: openai_api_rs::v1::types::JSONSchemaType::Object,
            properties: Some(properties),
            required: Some(vec!["path".to_string()]),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let path = args["path"].as_str().unwrap_or("").to_string();
        if path.is_empty() {
            return Err("path is required".to_string());
        }

        let chunk_tokens = args["chunk_tokens"].as_u64().unwrap_or(1024) as usize;
        let overlap_tokens = args["overlap_tokens"].as_u64().unwrap_or(128) as usize;

        let text = self.get_markdown_content(&path)?;
        if text.is_empty() {
            return Err("file is empty or could not be read".to_string());
        }

        let paragraphs = self.split_paragraphs_with_headings(text);
        let chunks = self.chunk_paragraphs(paragraphs, chunk_tokens, overlap_tokens);

        serde_json::to_string(&chunks).map_err(|e| e.to_string())
    }
}
