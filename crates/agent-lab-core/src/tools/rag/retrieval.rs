use std::collections::{HashMap, HashSet};

use crate::error::AgentLabError;
use crate::services::rag_service::{RagChunk, RagService};

/// 语义检索：把 query 向量化后在 `rag_chunks` 中搜索最近的 chunk。
///
/// 如果启用了 MQE（默认启用），会先用子 agent 把 query 扩展成多个等价查询句，
/// 分别检索后合并、去重、按最佳相似度重排，提升召回率。
/// HyDE 会同时生成假设文档并加入检索集合。
pub async fn search(
    service: &RagService,
    query: &str,
    namespace: Option<&str>,
    limit: usize,
) -> Result<Vec<(f64, RagChunk)>, AgentLabError> {
    // 1. HyDE 任务与 MQE 任务并发执行，各自负责增强 + 检索
    let (hyde_results, mqe_results) = tokio::join!(
        hyde_search_task(service, query, namespace, limit),
        mqe_search_task(service, query, namespace, limit),
    );

    // 2. search 只负责合并两个任务返回的结果集
    let mut all_results = Vec::new();
    all_results.extend(hyde_results);
    all_results.extend(mqe_results);

    // 3. 按 chunk id 去重，保留最大 similarity
    let mut best: HashMap<String, (f64, RagChunk)> = HashMap::new();
    for (score, chunk) in all_results {
        best.entry(chunk.id.clone())
            .and_modify(|(s, _)| {
                if score > *s {
                    *s = score;
                }
            })
            .or_insert((score, chunk));
    }

    // 4. 按 similarity 降序排列，取 top limit
    let mut merged: Vec<(f64, RagChunk)> = best.into_values().collect();
    merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    merged.truncate(limit);

    Ok(merged)
}

/// HyDE 增强检索任务。
///
/// 生成假设文档并立即检索；未启用 HyDE 或生成失败时返回空 Vec。
async fn hyde_search_task(
    service: &RagService,
    query: &str,
    namespace: Option<&str>,
    limit: usize,
) -> Vec<(f64, RagChunk)> {
    let Some(hyde) = &service.hyde_generator else {
        return Vec::new();
    };

    let mut guard = hyde.lock().await;
    let doc = match guard.generate(query).await {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("[RagIndex] HyDE generation failed: {}, skip", e);
            return Vec::new();
        }
    };

    eprintln!(
        "[RagIndex] HyDE generated: {}",
        doc.chars().take(80).collect::<String>()
    );

    match service.search_single(&doc, namespace, limit).await {
        Ok(results) => results,
        Err(e) => {
            eprintln!("[RagIndex] HyDE search failed: {}, skip", e);
            Vec::new()
        }
    }
}

/// MQE 扩展检索任务。
///
/// 对原始 query 做扩展，连同原始 query 一起并发检索；
/// 未启用 MQE 时只检索原始 query。
async fn mqe_search_task(
    service: &RagService,
    query: &str,
    namespace: Option<&str>,
    limit: usize,
) -> Vec<(f64, RagChunk)> {
    let mut search_queries = vec![query.to_string()];

    if let Some(expander) = &service.query_expander {
        let mut guard = expander.lock().await;
        match guard.expand(query).await {
            Ok(qs) => search_queries.extend(qs),
            Err(e) => {
                eprintln!("[RagIndex] MQE expansion failed: {}, use original query", e);
            }
        }
    }

    // 去重，避免重复检索
    let mut seen = HashSet::new();
    search_queries.retain(|q| seen.insert(q.clone()));

    let futures = search_queries
        .iter()
        .map(|q| service.search_single(q, namespace, limit));

    match futures_util::future::try_join_all(futures).await {
        Ok(results_per_query) => results_per_query.into_iter().flatten().collect(),
        Err(e) => {
            eprintln!("[RagIndex] MQE search failed: {}, skip", e);
            Vec::new()
        }
    }
}
