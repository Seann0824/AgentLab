use std::env;
use anyhow;
use dotenvy;
use reqwest;
use serde;
use serde_json;


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let api_key = env::var("DEEPSEEK_API_KEY")?;
    let deepseek_base_url = env::var("DEEPSEEK_BASE_URL")?;

    // 1. 确认配置文件读取成功
    println!("API Key: {}", api_key);
    println!("DeepSeek Base URL: {}", deepseek_base_url);

    // 2. 尝试通过cli调用模型
    let models = ["deepseek-v4-flash", "deepseek-v4-pro"];

    let client = reqwest::Client::new();
    let res = client.post(format!("{}/chat/completions", deepseek_base_url))
    .bearer_auth(api_key)
    .json(&serde_json::json!({
        "model": "deepseek-v4-flash",
        "messages": [
            {
                "role": "user",
                "content": "你好，我是DeepSeek，一个智能助手。"
            }
        ]
    }))
    .send()
    .await?;

    let status = res.status();
    let body = res.text().await?;
    println!("Status: {}", status);
    println!("Body: {}", body);

    Ok(())
}
