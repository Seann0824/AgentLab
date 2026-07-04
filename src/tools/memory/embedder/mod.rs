
mod ollama_embedder;

pub use ollama_embedder::OllamaEmbedder;

#[async_trait::async_trait]
pub trait Embedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, String>;
}


