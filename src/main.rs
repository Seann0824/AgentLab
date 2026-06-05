use std::env;
use anyhow;
use dotenvy;
use reqwest;
use serde_json;
use futures_util::StreamExt;

use crate::model::ChatMessage;

mod model;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let query_client = initial_model()?;
    let messages = vec![
        ChatMessage {
            role: "user".to_string(),
            content: "直接输出\n换行，是字符".to_string(),
        }
    ];
    query_client.stream_chat(messages).await?;
    Ok(())
}


fn initial_model() -> anyhow::Result<Box<dyn model::ModelAdapter>> {
    // 1. 读取环境变量
    dotenvy::dotenv().ok();

    let api_key = env::var("DEEPSEEK_API_KEY").unwrap();
    let deepseek_base_url = env::var("DEEPSEEK_BASE_URL").unwrap();

    let openai_adapter = 
        model::OpenAiCompatibleAdapter::new(
            deepseek_base_url, 
            api_key, 
            "deepseek-v4-flash".to_string()
        );
    
    Ok(Box::new(openai_adapter))
}
