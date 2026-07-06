use std::collections::HashMap;

use crate::tools::rag::index::{RagChunk, RagIndex};

/// 语义检索：把 query 向量化后在 `rag_chunks` 中搜索最近的 chunk。
///
/// 如果启用了 MQE（默认启用），会先用子 agent 把 query 扩展成多个等价查询句，
/// 分别检索后合并、去重、按最佳相似度重排，提升召回率。
/// HyDE 会同时生成假设文档并加入检索集合。
pub async fn search(
    index: &RagIndex,
    query: &str,
    namespace: Option<&str>,
    limit: usize,
) -> Result<Vec<(f64, RagChunk)>, String> {
    // 1. 收集原始查询与 HyDE 假设文档
    let mut base_queries = vec![query.to_string()];
    if let Some(hyde) = &index.hyde_generator {
        let mut guard = hyde.lock().await;
        match guard.generate(query).await {
            Ok(doc) => {
                eprintln!(
                    "[RagIndex] HyDE generated: {}",
                    doc.chars().take(80).collect::<String>()
                );
                base_queries.push(doc);
            }
            Err(e) => {
                eprintln!(
                    "[RagIndex] HyDE generation failed: {}, use original query only",
                    e
                );
            }
        }
    }

    // 2. 用 MQE 对每个 base query 做扩展
    let mut expanded_queries: Vec<String> = Vec::new();
    match &index.query_expander {
        Some(expander) => {
            let mut guard = expander.lock().await;
            for base in base_queries {
                match guard.expand(&base).await {
                    Ok(mut qs) => expanded_queries.append(&mut qs),
                    Err(e) => {
                        eprintln!(
                            "[RagIndex] MQE expansion failed: {}, use base query",
                            e
                        );
                        expanded_queries.push(base);
                    }
                }
            }
        }
        None => expanded_queries = base_queries,
    }

    // 去重，避免重复检索
    let mut seen = std::collections::HashSet::new();
    expanded_queries.retain(|q| seen.insert(q.clone()));

    // 3. 对每个查询分别检索
    let mut all_results: Vec<(f64, RagChunk)> = Vec::new();
    for q in expanded_queries {
        let results = index.search_single(&q, namespace, limit).await?;
        all_results.extend(results);
    }

    // 4. 按 chunk id 去重，保留最佳（最小）distance
    let mut best: HashMap<String, (f64, RagChunk)> = HashMap::new();
    for (score, chunk) in all_results {
        best.entry(chunk.id.clone())
            .and_modify(|(s, _)| {
                if score < *s {
                    *s = score;
                }
            })
            .or_insert((score, chunk));
    }

    // 5. 按 distance 升序排列，取 top limit
    let mut merged: Vec<(f64, RagChunk)> = best.into_values().collect();
    merged.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    merged.truncate(limit);

    Ok(merged)
}
