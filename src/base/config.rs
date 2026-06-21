use std::env;

pub struct Config {
    default_model: String,
    default_provider: String,
    temperature: f64,
    max_tokens: Option<u64>,

    // 系统配置
    debug: bool,
    log_level: String,

    // 其他配置
    max_history_length: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_model: "deepseek-v4-flash".into(),
            default_provider: "DeepSeek".into(),
            temperature: 0.7,
            max_tokens: None,

            // 系统配置
            debug: false,
            log_level: "INFO".into(),

            // 其他配置
            max_history_length: 100,
        }
    }
}

impl Config {
    pub fn from_env() -> Config {
        dotenvy::dotenv().ok();
        let mut config = Config::default();
        if let Ok(default_model) = env::var("DEFAULT_MODEL") {
            config.default_model = default_model;
        }
        if let Ok(default_provider) = env::var("DEFAULT_PROVIDER") {
            config.default_provider = default_provider;
        }
        if let Ok(debug) = env::var("DEBUG") {
            config.debug = debug == "true";
        }
        if let Ok(temperature) = env::var("TEMPERATURE") {
            config.temperature = temperature.parse().unwrap();
        }
        if let Ok(max_tokens) = env::var("MAX_TOKENS") {
            config.max_tokens = Some(max_tokens.parse().unwrap_or_default());
        }

        config
    }
}