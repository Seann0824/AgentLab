use tokio::sync::mpsc;
use futures_util::StreamExt;
use serde_json::json;
use tokio_stream::wrappers::ReceiverStream;

use crate::model::{ModelEvent, types::ModelStream};

use super::{ChatMessage, ModelAdapter};

pub struct OpenAiCompatibleAdapter {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleAdapter {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

impl ModelAdapter for OpenAiCompatibleAdapter {
    fn stream_chat(&self, messages: Vec<ChatMessage>) -> ModelStream {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        
        let (tx, rx) = mpsc::channel(100);

        // 感觉应该先将 messages 做转换，不过这里看着如果所有模型格式都统一的话无所
        let query_model = self.client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": &self.model,
                "stream": true,
                "messages": &messages,
            }))
            .send();

        tokio::spawn(async move {
            match query_model.await {
                Result::Ok(stream) => {
                    let mut stream = stream.bytes_stream();
                    let mut buffer = String::new();
                    while let Some(chunck) = stream.next().await {
                        if let Result::Ok(bytes) = chunck {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }

                        while let Some(pos) = buffer.find('\n') {
                            let mut line = buffer[..pos].to_string();
                            buffer.drain(..pos + 1);
                            line = line.trim_end_matches('\r').to_string();
                            if line.is_empty() {
                                continue
                            }

                            let Some(mut data) = line.strip_prefix("data: ") else {
                                continue
                            };
                            
                            data = data.trim();
                            if data == "[DONE]" {
                                return;
                            }

                            match serde_json::from_str::<serde_json::Value>(data) {
                                Result::Ok(value) => {
                                    if let Some(content) = value["choices"][0]["delta"]["content"].as_str() {
                                        tx.send(ModelEvent::Text(content.to_string())).await;
                                    }

                                    if let Some(reasoning_content) = value["choices"][0]["delta"]["reasoning_content"].as_str() {
                                        tx.send(ModelEvent::Thinking(reasoning_content.to_string())).await;
                                    }
                                },
                                Err(_) => {
                                    tx.send(ModelEvent::Error("json parse error".to_string())).await;
                                }
                            }
                        }
                    }
                },
                Err(_) => {
                    tx.send(ModelEvent::Error("request error".to_string())).await;
                }
            }
        });
        
        Box::pin(ReceiverStream::new(rx))
    }
}
