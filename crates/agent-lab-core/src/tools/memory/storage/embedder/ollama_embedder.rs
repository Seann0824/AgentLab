use std::env;

use super::Embedder;
use reqwest::Client;
use serde_json::json;

pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    client: Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        dotenvy::dotenv().ok();
        let base_url = base_url.unwrap_or(env::var("EMBEDDER_URL")
            .unwrap_or("http://localhost:11434/api/embeddings".into()));
        let client = Client::new();
        let model = model.unwrap_or(env::var("EMBEDDER_MODEL")
            .unwrap_or("nomic-embed-text".into()));
        Self {
            client,
            base_url,
            model,
        }
    }
}

#[async_trait::async_trait]
impl Embedder for OllamaEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, String> {
        let resp = self.client
            .post(&self.base_url)
            .json(&json!({
                "model": self.model,
                "prompt": text,
            }))
            .send()
            .await;
        let Ok(resp) = resp else {
            return Err("fetch error for OllamaEmbedder".into());
        };

        let embedding = match resp.json::<serde_json::Value>().await {
            Ok(value) => {
                let Ok(embedding) = serde_json::from_value::<Vec<f32>>(value["embedding"].clone()) else {
                    return Err(format!("[OllamaEmbedder] json parser error"));
                };
                embedding
            },
            Err(msg) => return Err(format!("[OllamaEmbedder] json parser error: {}", msg.to_string())),
        };

        Ok(embedding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_embedder() {
        let embedder = OllamaEmbedder::new(None, None);
        assert_eq!(embedder.base_url, "http://localhost:11434/api/embeddings");
        assert_eq!(embedder.model, "nomic-embed-text");
    }
}
