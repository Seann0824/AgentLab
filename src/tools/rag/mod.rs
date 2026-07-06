use std::{collections::HashMap, sync::Arc, sync::OnceLock};

use pgvector::Vector;
use regex::Regex;
use scirs2_text::is_cjk_char;
use sqlx::{PgPool, Row};

use crate::tools::memory::storage::embedder::Embedder;
use crate::tools::memory::storage::OllamaEmbedder;
use crate::tools::types::Tool;

pub struct RagTool {
    index: Option<RagIndex>,
}

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct Paragraph {
    pub content: String,
    pub heading_path: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// RAG 全局资料库中的一条 chunk 记录。
/// 与 memory 的 `memories` 表解耦，字段更精简，面向文档检索场景。
#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct RagChunk {
    pub id: String,
    pub namespace: String,
    pub source: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub heading_path: Option<String>,
    pub start: usize,
    pub end: usize,
    pub chunk_index: usize,
    pub metadata: serde_json::Value,
}

impl RagTool {
    /// 创建一个不带索引器的 RagTool，仅用于分块、预处理和 token 估算等本地操作。
    pub fn new() -> Self {
        Self { index: None }
    }

    /// 使用已有的 RagIndex 创建 RagTool。
    pub fn with_index(index: RagIndex) -> Self {
        Self { index: Some(index) }
    }

    /// 便捷方法：用默认 Ollama embedder + PG 连接创建 RagTool。
    pub fn with_default_embedder(db: PgPool) -> Self {
        Self::with_index(RagIndex::with_default_embedder(db))
    }

    fn index(&self) -> Result<&RagIndex, String> {
        self.index
            .as_ref()
            .ok_or_else(|| "RagTool 未初始化索引器".to_string())
    }

    /// 把本地 Markdown 文件索引到 `rag_chunks`。
    pub async fn index_file(
        &self,
        path: &str,
        namespace: &str,
        chunk_tokens: usize,
        overlap_tokens: usize,
    ) -> Result<usize, String> {
        let text = self.get_markdown_content(path)?;
        if text.is_empty() {
            return Err("file is empty or could not be read".to_string());
        }

        let paragraphs = self.split_paragraphs_with_headings(text);
        let chunks = self.chunk_paragraphs(paragraphs, chunk_tokens, overlap_tokens);
        let count = chunks.len();

        let index = self.index()?;
        index
            .clear_namespace(namespace)
            .await
            .map_err(|e| format!("clear namespace failed: {}", e))?;
        index
            .index_chunks(chunks, path, namespace, 8)
            .await
            .map_err(|e| format!("index chunks failed: {}", e))?;

        Ok(count)
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

    pub fn preprocess_markdown_for_embedding(&self, content: &str) -> String {
        preprocess_markdown_for_embedding(content)
    }
}

/// 预处理 Markdown 文本，去掉标记符号但保留语义内容，用于生成更干净的 embedding。
pub fn preprocess_markdown_for_embedding(content: &str) -> String {
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

#[async_trait::async_trait]
impl Tool for RagTool {
    fn name(&self) -> &str {
        "rag"
    }

    fn description(&self) -> &str {
        "RAG 资料库工具：支持 add_document（索引 Markdown 文档）和 search（语义检索）。"
    }

    fn parameters_schema(&self) -> openai_api_rs::v1::types::FunctionParameters {
        let mut properties = HashMap::new();
        properties.insert(
            "action".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("操作类型：add_document 或 search".to_string()),
                ..Default::default()
            }),
        );
        properties.insert(
            "file_path".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("add_document 时必填：Markdown 文件路径".to_string()),
                ..Default::default()
            }),
        );
        properties.insert(
            "query".to_string(),
            Box::new(openai_api_rs::v1::types::JSONSchemaDefine {
                schema_type: Some(openai_api_rs::v1::types::JSONSchemaType::String),
                description: Some("search 时必填：查询问题".to_string()),
                ..Default::default()
            }),
        );
        openai_api_rs::v1::types::FunctionParameters {
            schema_type: openai_api_rs::v1::types::JSONSchemaType::Object,
            properties: Some(properties),
            required: Some(vec!["action".to_string()]),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().unwrap_or("").to_string();

        match action.as_str() {
            "add_document" => {
                let path = args["file_path"].as_str().unwrap_or("").to_string();
                if path.is_empty() {
                    return Err("add_document 操作需要 file_path 参数".to_string());
                }

                // 内部固定参数：整个 RAG 视为统一数据库
                let count = self
                    .index_file(&path, "default", 512, 64)
                    .await?;
                Ok(format!("索引完成，共 {} 个 chunk", count))
            }
            "search" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                if query.is_empty() {
                    return Err("search 操作需要 query 参数".to_string());
                }

                let index = self.index()?;
                // 不限制 namespace，在整个 RAG 数据库里按向量语义相似度排序
                let results = index.search(&query, None, 5).await?;

                let formatted: Vec<String> = results
                    .iter()
                    .enumerate()
                    .map(|(i, (score, chunk))| {
                        format!(
                            "[{}] distance={:.3}\n来源: {}\n标题路径: {}\n{}",
                            i + 1,
                            score,
                            chunk.source,
                            chunk.heading_path.as_deref().unwrap_or("无"),
                            chunk.content
                        )
                    })
                    .collect();

                Ok(format!(
                    "检索到 {} 条相关 chunk：\n\n{}",
                    formatted.len(),
                    formatted.join("\n\n")
                ))
            }
            _ => Err(format!("不支持的 action: {}", action)),
        }
    }
}

/// RAG 索引器：负责把分块后的文本生成 embedding 并写入独立的 `rag_chunks` 表。
///
/// 与 memory 体系解耦：`rag_chunks` 是全局资料库，字段精简，
/// namespace 仅用于多资料库隔离，不对应具体用户。
#[derive(Clone)]
pub struct RagIndex {
    db: PgPool,
    embedder: Arc<dyn Embedder + Send + Sync>,
    dimension: usize,
}

impl RagIndex {
    pub fn new(db: PgPool, embedder: Arc<dyn Embedder + Send + Sync>, dimension: usize) -> Self {
        Self {
            db,
            embedder,
            dimension,
        }
    }

    /// 使用默认的 Ollama embedder 创建索引器。
    /// 默认维度 768，与 `init_pg.sql` 中的 rag_chunks.embedding VECTOR(768) 对应。
    pub fn with_default_embedder(db: PgPool) -> Self {
        let embedder = Arc::new(OllamaEmbedder::new(None, None));
        Self::new(db, embedder, 768)
    }

    /// 将 RAG chunk 预处理后生成 embedding，批量写入 `rag_chunks`。
    ///
    /// - `source`: 来源文档标识（如文件路径）
    /// - `namespace`: RAG 命名空间，用于多资料库隔离
    /// - `batch_size`: 每批写入的 chunk 数量
    pub async fn index_chunks(
        &self,
        chunks: Vec<Paragraph>,
        source: &str,
        namespace: &str,
        batch_size: usize,
    ) -> Result<(), String> {
        if chunks.is_empty() {
            return Ok(());
        }

        let processed: Vec<(Paragraph, String)> = chunks
            .into_iter()
            .map(|chunk| {
                let processed = preprocess_markdown_for_embedding(&chunk.content);
                (chunk, processed)
            })
            .collect();

        let mut batch: Vec<RagChunk> = Vec::with_capacity(batch_size);

        for (chunk_index, (chunk, text)) in processed.into_iter().enumerate() {
            let mut embedding = self
                .embedder
                .encode(&text)
                .await
                .map_err(|e| format!("[RagIndex] embedding failed: {}", e))?;
            self.normalize_embedding(&mut embedding);

            batch.push(RagChunk {
                id: uuid::Uuid::new_v4().to_string(),
                namespace: namespace.to_string(),
                source: source.to_string(),
                content: text,
                embedding,
                heading_path: chunk.heading_path,
                start: chunk.start,
                end: chunk.end,
                chunk_index,
                metadata: serde_json::json!({
                    "original_content": chunk.content,
                }),
            });

            if batch.len() >= batch_size {
                self.insert_batch(&batch).await?;
                batch.clear();
            }
        }

        if !batch.is_empty() {
            self.insert_batch(&batch).await?;
        }

        Ok(())
    }

    /// 语义检索：把 query 向量化后在 `rag_chunks` 中搜索最近的 chunk。
    pub async fn search(
        &self,
        query: &str,
        namespace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(f64, RagChunk)>, String> {
        let mut embedding = self
            .embedder
            .encode(query)
            .await
            .map_err(|e| format!("[RagIndex] query embedding failed: {}", e))?;
        self.normalize_embedding(&mut embedding);
        let pg_vector = Vector::from(embedding);

        let rows = sqlx::query(
            r#"
            SELECT
                id, namespace, source, content, embedding,
                heading_path, start_pos, end_pos, chunk_index, metadata,
                embedding <=> $1 AS score
            FROM rag_chunks
            WHERE namespace = $2 OR $2 IS NULL
            ORDER BY embedding <=> $1
            LIMIT $3
            "#,
        )
        .bind(pg_vector)
        .bind(namespace)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await
        .map_err(|e| format!("[RagIndex] search failed: {}", e))?;

        let results = rows
            .into_iter()
            .map(|row| {
                let chunk = RagChunk {
                    id: row.get("id"),
                    namespace: row.get("namespace"),
                    source: row.get("source"),
                    content: row.get("content"),
                    embedding: row
                        .get::<Option<Vector>, _>("embedding")
                        .map(|v| v.to_vec())
                        .unwrap_or_default(),
                    heading_path: row.get("heading_path"),
                    start: row.get::<i64, _>("start_pos") as usize,
                    end: row.get::<i64, _>("end_pos") as usize,
                    chunk_index: row.get::<i32, _>("chunk_index") as usize,
                    metadata: row
                        .get::<Option<serde_json::Value>, _>("metadata")
                        .unwrap_or_else(|| serde_json::json!({})),
                };
                let score: f64 = row.get("score");
                (score, chunk)
            })
            .collect();

        Ok(results)
    }

    fn normalize_embedding(&self, embedding: &mut Vec<f32>) {
        if embedding.len() < self.dimension {
            embedding.extend(vec![0.0f32; self.dimension - embedding.len()]);
        } else if embedding.len() > self.dimension {
            embedding.truncate(self.dimension);
        }
    }

    /// 清空某个 namespace 下的所有 chunk，便于重新索引同一资料库。
    pub async fn clear_namespace(&self, namespace: &str) -> Result<u64, String> {
        let deleted = sqlx::query("DELETE FROM rag_chunks WHERE namespace = $1")
            .bind(namespace)
            .execute(&self.db)
            .await
            .map_err(|e| format!("[RagIndex] clear namespace failed: {}", e))?
            .rows_affected();
        Ok(deleted)
    }

    async fn insert_batch(&self, batch: &[RagChunk]) -> Result<(), String> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(|e| format!("[RagIndex] transaction begin failed: {}", e))?;

        for chunk in batch {
            let pg_vector = Vector::from(chunk.embedding.clone());
            sqlx::query(
                r#"
                INSERT INTO rag_chunks (
                    id, namespace, source, content, embedding,
                    heading_path, start_pos, end_pos, chunk_index, metadata
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                "#,
            )
            .bind(&chunk.id)
            .bind(&chunk.namespace)
            .bind(&chunk.source)
            .bind(&chunk.content)
            .bind(pg_vector)
            .bind(&chunk.heading_path)
            .bind(chunk.start as i64)
            .bind(chunk.end as i64)
            .bind(chunk.chunk_index as i32)
            .bind(&chunk.metadata)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("[RagIndex] insert failed: {}", e))?;
        }

        tx.commit()
            .await
            .map_err(|e| format!("[RagIndex] transaction commit failed: {}", e))?;

        Ok(())
    }
}
