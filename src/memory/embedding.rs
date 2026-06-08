// src/memory/embedding.rs
//
// EmbeddingClient — 调用 LLM API 的 /embeddings 端点生成文本向量。
//
// 复用 ModelManager 的配置（base_url, api_key），通过环境变量自动发现。
// 默认使用 text-embedding-3-small 模型，维度 1536。

use std::env;

/// Embedding 客户端
pub struct EmbeddingClient {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
    vector_dim: usize,
}

impl EmbeddingClient {
    /// 创建 EmbeddingClient
    ///
    /// 自动从环境变量读取配置：
    /// - LLM_BASE_URL: API 基础地址
    /// - LLM_API_KEY: API Key
    /// - LLM_EMBEDDING_MODEL: 嵌入模型名（默认 text-embedding-3-small）
    /// - LLM_EMBEDDING_DIM: 向量维度（默认 1536）
    pub fn from_env() -> anyhow::Result<Self> {
        let base_url = env::var("LLM_BASE_URL")
            .map_err(|_| anyhow::anyhow!("LLM_BASE_URL not set"))?;
        let api_key = env::var("LLM_API_KEY")
            .map_err(|_| anyhow::anyhow!("LLM_API_KEY not set"))?;
        let model = env::var("LLM_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".to_string());
        let vector_dim = env::var("LLM_EMBEDDING_DIM")
            .ok()
            .and_then(|d| d.parse::<usize>().ok())
            .unwrap_or(1536);

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            client: reqwest::Client::new(),
            vector_dim,
        })
    }

    /// 从已有的 base_url、api_key 创建（复用 ModelManager 的配置）
    pub fn new(base_url: &str, api_key: &str) -> Self {
        let model = env::var("LLM_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".to_string());
        let vector_dim = env::var("LLM_EMBEDDING_DIM")
            .ok()
            .and_then(|d| d.parse::<usize>().ok())
            .unwrap_or(1536);

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model,
            client: reqwest::Client::new(),
            vector_dim,
        }
    }

    /// 生成单个文本的嵌入向量
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": text,
        });

        let resp = self.client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Embedding API error ({}): {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let vector = data["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response: missing 'data[0].embedding'"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(vector)
    }

    /// 批量生成嵌入向量
    pub async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.base_url);
        let inputs: Vec<&str> = texts.iter().map(|s| *s).collect();
        let body = serde_json::json!({
            "model": self.model,
            "input": inputs,
        });

        let resp = self.client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Embedding API error ({}): {}", status, text));
        }

        let data: serde_json::Value = resp.json().await?;
        let mut results = Vec::new();
        if let Some(arr) = data["data"].as_array() {
            // Sort by index to maintain order
            let mut sorted = arr.clone();
            sorted.sort_by(|a, b| {
                let ia = a["index"].as_u64().unwrap_or(0);
                let ib = b["index"].as_u64().unwrap_or(0);
                ia.cmp(&ib)
            });
            for item in &sorted {
                let vector = item["embedding"]
                    .as_array()
                    .map(|arr| arr.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect())
                    .unwrap_or_default();
                results.push(vector);
            }
        }

        Ok(results)
    }

    /// 获取向量维度
    pub fn vector_dim(&self) -> usize {
        self.vector_dim
    }

    /// 获取模型名
    pub fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 EmbeddingClient 创建（不实际调用 API）
    #[test]
    fn test_embedding_client_creation() {
        let client = EmbeddingClient::new("https://api.openai.com/v1", "test-key");
        assert_eq!(client.vector_dim, 1536);
        assert_eq!(client.model, "text-embedding-3-small");
        assert_eq!(client.base_url, "https://api.openai.com/v1");
    }

    /// 测试空文本的处理
    #[tokio::test]
    async fn test_embed_empty_text() {
        let client = EmbeddingClient::new("https://api.openai.com/v1", "test-key");
        // This should fail because API is not reachable
        let result = client.embed("").await;
        assert!(result.is_err(), "Expected error for unreachable API");
    }
}
