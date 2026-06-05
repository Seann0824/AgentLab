use anyhow::Ok;
use futures_util::StreamExt;
use serde_json::json;
use serde;
use bytes::{Buf, buf};

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


const PREFIX: usize = 6;

#[async_trait::async_trait]
impl ModelAdapter for OpenAiCompatibleAdapter {
    async fn stream_chat(&self, messages: Vec<ChatMessage>) -> anyhow::Result<()> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        // 感觉应该先将 messages 做转换，不过这里看着如果所有模型格式都统一的话无所

        let res = self.client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": &self.model,
                "stream": true,
                "messages": &messages,
            }))
            .send()
            .await?;
        
        let mut stream = res.bytes_stream();
        let mut buffer = String::new();
        // 将流取出来，并向外部抛出去，然后外部在来个工具解析这个流
        while let Some(chunck) = stream.next().await {
            let chunck = chunck?;
            // 判断当前是否有一个合法的 Block，先将数据加入缓存，然后判断当前缓存是否有一个合法的block块
            // 跳过前缀
            // let chunck = &chunck[PREFIX..]; 
            buffer.push_str(&String::from_utf8_lossy(&chunck));

            // println!("===");
            // println!("{}", &String::from_utf8_lossy(&chunck));
            // println!("===");
            
            while let Some(pos) = buffer.find('\n') {
                let mut line = buffer[..pos].to_string();
                line = line.trim_end_matches('\r').to_string();

                buffer.drain(..pos + 1);
                if line.is_empty() {
                    continue;
                }

                let Some(data) = line.strip_prefix("data: ") else {
                    continue
                };
                let data = data.trim();
                if data == "[DONE]" {
                    break;
                }

                let response = serde_json::from_str::<serde_json::Value>(data)?;
                if let Some(content) = response["choices"][0]["delta"]["content"].as_str() {
                    print!("{}", content);
                    std::io::Write::flush(&mut std::io::stdout())?;
                }

                if let Some(reasoning_content) = response["choices"][0]["delta"]["reasoning_content"].as_str() {
                    print!("\x1b[90m{}\x1b[0m", reasoning_content);
                    std::io::Write::flush(&mut std::io::stdout())?;
                }

            }
        }
        Ok(())
    }
}
