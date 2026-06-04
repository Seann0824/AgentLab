use std::env;
use anyhow;
use dotenvy;
fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let api_key = env::var("DEEPSEEK_API_KEY")?;
    let deepseek_base_url = env::var("DEEPSEEK_BASE_URL")?;

    // 1. 确认配置文件读取成功
    println!("API Key: {}", api_key);
    println!("DeepSeek Base URL: {}", deepseek_base_url);

    Ok(())
}
