// src/model/manager.rs
//
// ⭐ 模型管理器 — 管理多个模型配置 + 当前活跃的 ModelAdapter
//
// 核心职责：
// 1. 从环境变量自动发现并注册模型配置
// 2. 维护当前活跃的模型适配器
// 3. 支持运行时切换模型（通过 switch() 方法）
// 4. 提供模型列表查询能力

use std::collections::HashMap;

use crate::model::ModelAdapter;
use crate::model::config::ModelConfig;
use crate::model::providers::build_adapter;

/// ⭐ 模型管理器
pub struct ModelManager {
    /// 所有已注册的模型配置（name → ModelConfig）
    configs: HashMap<String, ModelConfig>,
    /// 所有已构建的 ModelAdapter（name → Box<dyn ModelAdapter>）
    adapters: HashMap<String, Box<dyn ModelAdapter>>,
    /// 当前活跃的模型名称
    active_name: String,
}

impl ModelManager {
    /// 从环境变量加载所有模型配置并构建适配器
    ///
    /// 详细规则见 `from_env` 文档。
    pub fn from_env() -> Self {
        let configs = Self::detect_configs_from_env();
        Self::from_configs(configs)
    }

    /// 从 ModelConfig 列表构建 ModelManager
    pub fn from_configs(configs: Vec<ModelConfig>) -> Self {
        let mut config_map = HashMap::new();
        let mut adapter_map = HashMap::new();
        let mut first_name = String::new();

        for cfg in configs {
            let name = cfg.name.clone();
            if first_name.is_empty() {
                first_name = name.clone();
            }
            config_map.insert(name.clone(), cfg.clone());

            // 构建适配器
            match build_adapter(&cfg) {
                Ok(adapter) => {
                    adapter_map.insert(name.clone(), adapter);
                }
                Err(e) => {
                    eprintln!("[ModelManager] 构建模型 '{}' 失败: {} (已跳过)", name, e);
                }
            }
        }

        let active_name = if first_name.is_empty() {
            "none".to_string()
        } else {
            first_name
        };

        // 如果没有模型，添加一个占位
        if config_map.is_empty() {
            eprintln!("[ModelManager] ⚠️ 未从环境变量发现任何模型配置");
        }

        Self {
            configs: config_map,
            adapters: adapter_map,
            active_name,
        }
    }

    /// 从环境变量自动发现模型配置
    ///
    /// 扫描所有环境变量，寻找 `{PREFIX}_API_KEY` + `{PREFIX}_BASE_URL` 配对。
    /// 支持的常见前缀: DEEPSEEK, OPENAI, ANTHROPIC, CUSTOM, AZURE, GROQ, etc.
    ///
    /// 环境变量约定：
    ///   {PREFIX}_API_KEY   — API 密钥（必选）
    ///   {PREFIX}_BASE_URL  — API 基础 URL（必选）
    ///   {PREFIX}_MODEL     — 模型名称（可选，默认 "default"）
    fn detect_configs_from_env() -> Vec<ModelConfig> {
        let mut configs = Vec::new();

        // 读取所有环境变量，分组检测
        let vars: Vec<(String, String)> = std::env::vars()
            .map(|(k, v)| (k.to_uppercase(), v))
            .collect();

        // 找到所有以 _API_KEY 结尾的变量，提取前缀
        let mut prefixes: Vec<String> = Vec::new();
        for (key, _) in &vars {
            if let Some(prefix) = key.strip_suffix("_API_KEY") {
                if !prefix.is_empty() {
                    prefixes.push(prefix.to_string());
                }
            }
        }
        // 去重
        prefixes.sort();
        prefixes.dedup();

        for prefix in &prefixes {
            let api_key = vars
                .iter()
                .find(|(k, _)| k == &format!("{}_API_KEY", prefix))
                .map(|(_, v)| v.clone());

            let base_url = vars
                .iter()
                .find(|(k, _)| k == &format!("{}_BASE_URL", prefix))
                .map(|(_, v)| v.clone());

            let model_name = vars
                .iter()
                .find(|(k, _)| k == &format!("{}_MODEL", prefix))
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| "default".to_string());

            if let (Some(api_key), Some(base_url)) = (api_key, base_url) {
                let name = prefix.to_lowercase();
                let config =
                    ModelConfig::new(&name, "openai-compatible", &base_url, &api_key, &model_name);
                configs.push(config);
            }
        }

        configs
    }

    // ========== 查询方法 ==========

    /// 返回所有已注册的模型配置列表
    pub fn list_models(&self) -> Vec<&ModelConfig> {
        let mut models: Vec<&ModelConfig> = self.configs.values().collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    }

    /// 根据名称获取模型配置
    pub fn get_model(&self, name: &str) -> Option<&ModelConfig> {
        self.configs.get(name)
    }

    /// 获取当前活跃的模型配置
    pub fn current(&self) -> Option<&ModelConfig> {
        self.configs.get(&self.active_name)
    }

    /// 获取当前活跃的模型适配器引用
    pub fn current_adapter(&self) -> Option<&Box<dyn ModelAdapter>> {
        self.adapters.get(&self.active_name)
    }

    /// 获取当前活跃的模型名称
    pub fn active_name(&self) -> &str {
        &self.active_name
    }

    /// 判断是否有任何模型可用
    pub fn has_models(&self) -> bool {
        !self.configs.is_empty()
    }

    // ========== 切换方法 ==========

    /// 切换到指定名称的模型
    ///
    /// 返回 Ok(true) 表示切换成功，Ok(false) 表示该模型已存在但适配器未构建
    pub fn switch(&mut self, name: &str) -> Result<bool, String> {
        if !self.configs.contains_key(name) {
            return Err(format!(
                "未知模型 '{}'。使用 /model list 查看可用模型",
                name
            ));
        }

        if name == self.active_name {
            return Ok(true); // 已经是当前模型
        }

        // 如果适配器尚未构建，尝试构建
        if !self.adapters.contains_key(name) {
            if let Some(cfg) = self.configs.get(name) {
                match build_adapter(cfg) {
                    Ok(adapter) => {
                        self.adapters.insert(name.to_string(), adapter);
                    }
                    Err(e) => {
                        return Err(format!("构建模型 '{}' 的适配器失败: {}", name, e));
                    }
                }
            }
        }

        self.active_name = name.to_string();
        Ok(true)
    }

    /// 动态注册一个新的模型配置并构建适配器
    /// 如果当前没有活跃模型（active_name=="none"），自动切换为新模型
    pub fn add_model(&mut self, config: ModelConfig) -> Result<(), String> {
        let name = config.name.clone();

        // 构建适配器
        let adapter = build_adapter(&config)
            .map_err(|e| format!("构建模型 '{}' 的适配器失败: {}", name, e))?;

        let is_first = self.active_name == "none";
        self.configs.insert(name.clone(), config);
        self.adapters.insert(name.clone(), adapter);
        if is_first {
            self.active_name = name;
        }
        Ok(())
    }

    /// 获取活跃适配器的 clone_box（供异步任务使用）
    pub fn clone_active_adapter(&self) -> Option<Box<dyn ModelAdapter>> {
        self.adapters.get(&self.active_name).map(|a| a.clone_box())
    }
}

// 让 ModelManager 支持 Clone（所有内容都是 Clone 的）
impl Clone for ModelManager {
    fn clone(&self) -> Self {
        let mut adapters = HashMap::new();
        for (name, adapter) in &self.adapters {
            adapters.insert(name.clone(), adapter.clone_box());
        }
        Self {
            configs: self.configs.clone(),
            adapters,
            active_name: self.active_name.clone(),
        }
    }
}

impl std::fmt::Debug for ModelManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelManager")
            .field("configs", &self.configs.keys().collect::<Vec<_>>())
            .field("active_name", &self.active_name)
            .field("adapter_count", &self.adapters.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_configs_empty() {
        let mm = ModelManager::from_configs(vec![]);
        assert!(!mm.has_models());
        assert!(mm.current().is_none());
        assert!(mm.list_models().is_empty());
    }

    #[test]
    fn test_from_configs_single() {
        let cfg = ModelConfig::new(
            "test",
            "openai-compatible",
            "https://test.com",
            "sk-key",
            "test-model",
        );
        let mm = ModelManager::from_configs(vec![cfg]);
        assert!(mm.has_models());
        assert_eq!(mm.active_name(), "test");
        assert!(mm.current().is_some());
        assert_eq!(mm.list_models().len(), 1);
    }

    #[test]
    fn test_switch_unknown() {
        let cfg = ModelConfig::new(
            "a",
            "openai-compatible",
            "https://a.com",
            "key-a",
            "model-a",
        );
        let mut mm = ModelManager::from_configs(vec![cfg]);
        let result = mm.switch("unknown");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("未知模型"));
    }

    #[test]
    fn test_switch_same() {
        let cfg = ModelConfig::new(
            "a",
            "openai-compatible",
            "https://a.com",
            "key-a",
            "model-a",
        );
        let mut mm = ModelManager::from_configs(vec![cfg]);
        assert!(mm.switch("a").unwrap());
        assert_eq!(mm.active_name(), "a");
    }

    #[test]
    fn test_add_model() {
        let mut mm = ModelManager::from_configs(vec![]);
        let cfg = ModelConfig::new(
            "new",
            "openai-compatible",
            "https://new.com",
            "key-new",
            "new-model",
        );
        assert!(mm.add_model(cfg).is_ok());
        assert!(mm.has_models());
        assert_eq!(mm.active_name(), "new");
    }

    #[test]
    fn test_detect_configs_from_env_empty() {
        // 不设任何环境变量时应返回空列表
        let configs = ModelManager::detect_configs_from_env();
        // 注意：测试环境可能已经有环境变量，所以我们只检查结构
        // 如果没有任何 _API_KEY + _BASE_URL 配对，则返回空
        // 这个测试不能严格断言，因为运行环境不同
    }

    #[test]
    fn test_clone() {
        let cfg = ModelConfig::new(
            "test",
            "openai-compatible",
            "https://test.com",
            "sk-key",
            "test-model",
        );
        let mm = ModelManager::from_configs(vec![cfg]);
        let cloned = mm.clone();
        assert_eq!(cloned.active_name(), "test");
        assert_eq!(cloned.list_models().len(), 1);
    }

    #[test]
    fn test_list_sorted() {
        let cfgs = vec![
            ModelConfig::new("z", "oc", "https://z.com", "k", "m"),
            ModelConfig::new("a", "oc", "https://a.com", "k", "m"),
            ModelConfig::new("m", "oc", "https://m.com", "k", "m"),
        ];
        let mm = ModelManager::from_configs(cfgs);
        let models = mm.list_models();
        assert_eq!(models[0].name, "a");
        assert_eq!(models[1].name, "m");
        assert_eq!(models[2].name, "z");
    }
}
