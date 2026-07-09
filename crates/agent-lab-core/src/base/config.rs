pub struct Config {
    // 系统配置
    debug: bool,
    log_level: String,

    // 其他配置
    max_history_length: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // 系统配置
            debug: false,
            log_level: "INFO".into(),

            // 其他配置
            max_history_length: 100,
        }
    }
}

impl Config {
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::new()
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn log_level(&self) -> &str {
        &self.log_level
    }

    pub fn max_history_length(&self) -> u64 {
        self.max_history_length
    }
}

pub struct ConfigBuilder {
    debug: bool,
    log_level: String,
    max_history_length: u64,
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigBuilder {
    pub fn new() -> Self {
        let config = Config::default();
        Self {
            debug: config.debug,
            log_level: config.log_level,
            max_history_length: config.max_history_length,
        }
    }

    pub fn debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    pub fn log_level(mut self, log_level: impl Into<String>) -> Self {
        self.log_level = log_level.into();
        self
    }

    pub fn max_history_length(mut self, max_history_length: u64) -> Self {
        self.max_history_length = max_history_length;
        self
    }

    pub fn build(self) -> Config {
        Config {
            debug: self.debug,
            log_level: self.log_level,
            max_history_length: self.max_history_length,
        }
    }
}
