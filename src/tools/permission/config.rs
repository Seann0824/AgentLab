//! 权限配置加载
//!
//! 支持两种配置来源：
//! 1. 默认配置（内嵌在代码中，开箱即用）
//! 2. 从 TOML 文件加载（支持自定义规则）
//!
//! 配置加载优先级：文件配置 > 默认配置（合并覆盖）

use std::path::Path;

use serde_json::Value;

use super::rules::PermissionConfig;

/// 权限配置加载器
pub struct PermissionConfigLoader;

impl PermissionConfigLoader {
    /// 从项目根目录加载配置
    ///
    /// 加载顺序：
    /// 1. 先使用默认配置
    /// 2. 如果存在 `config.toml` 文件，从中读取配置并合并
    ///
    /// # 参数
    /// - `project_root`: 项目根目录，用于路径规则解析和配置文件查找
    pub fn load(project_root: &str) -> PermissionConfig {
        let mut config = PermissionConfig::default_for_project(project_root);

        // 尝试从配置文件加载额外规则
        let config_path = Path::new(project_root).join("src/tools/permission/config.toml");
        if config_path.exists() {
            match Self::load_from_file(&config_path) {
                Ok(file_config) => {
                    // 合并文件配置中的规则到默认配置
                    if !file_config.path_rules.is_empty() {
                        config.path_rules = file_config.path_rules;
                    }
                    if !file_config.command_rules.is_empty() {
                        config.command_rules = file_config.command_rules;
                    }
                    if !file_config.sensitive_file_rules.is_empty() {
                        config.sensitive_file_rules = file_config.sensitive_file_rules;
                    }
                }
                Err(e) => {
                    eprintln!("⚠️  加载权限配置文件失败: {} (将使用默认配置)", e);
                }
            }
        }

        config
    }

    /// 从 TOML 文件加载配置
    fn load_from_file(path: &Path) -> anyhow::Result<PermissionConfig> {
        let content = std::fs::read_to_string(path)?;
        let config: PermissionConfig = basic_toml_parse(&content)?;
        Ok(config)
    }
}

/// 简单的 TOML 解析器（用于解析权限配置文件）
///
/// 由于不想引入额外的 TOML 解析依赖，实现了这个轻量级解析器。
/// 支持的 TOML 语法子集：
/// - 键值对: `key = "value"`
/// - 数组: `[[array_name]]`
/// - 字符串、布尔值、整数
/// - 注释: `# comment`
///
/// 不支持：嵌套表、多行字符串、日期时间等复杂语法
fn basic_toml_parse(content: &str) -> anyhow::Result<PermissionConfig> {
    use serde::Deserialize;

    let mut json_value = serde_json::Map::new();

    // 按行解析
    let lines: Vec<&str> = content.lines().collect();
    let mut current_array: Option<String> = None;
    let mut current_array_items: Vec<serde_json::Map<String, Value>> = Vec::new();
    let mut current_item: Option<serde_json::Map<String, Value>> = None;

    for line in lines {
        let line = line.trim();

        // 跳过空行和注释
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // 解析数组开始 [[...]]
        if line.starts_with("[[") {
            if let Some(end) = line.find("]]") {
                // 保存前一个数组项
                if let Some(item) = current_item.take() {
                    current_array_items.push(item);
                }
                // 保存前一个数组
                if let Some(ref arr_name) = current_array {
                    if !current_array_items.is_empty() {
                        let arr_values: Vec<Value> = current_array_items
                            .drain(..)
                            .map(Value::Object)
                            .collect();
                        json_value.insert(arr_name.clone(), Value::Array(arr_values));
                    }
                }

                let start = line.find("[[").unwrap() + 2;
                current_array = Some(line[start..end].to_string());
                current_array_items = Vec::new();
                current_item = None;
                continue;
            }
        }

        // 解析数组内的键值对
        if current_array.is_some() {
            if let Some((key, val)) = parse_kv_pair(line) {
                if current_item.is_none() {
                    current_item = Some(serde_json::Map::new());
                }
                if let Some(ref mut item) = current_item {
                    item.insert(key, val);
                }
            }
            continue;
        }

        // 解析顶层键值对
        if let Some((key, val)) = parse_kv_pair(line) {
            json_value.insert(key, val);
        }
    }

    // 保存最后的数组项
    if let Some(item) = current_item.take() {
        current_array_items.push(item);
    }
    if let Some(ref arr_name) = current_array {
        if !current_array_items.is_empty() {
            let arr_values: Vec<Value> = current_array_items
                .drain(..)
                .map(Value::Object)
                .collect();
            json_value.insert(arr_name.clone(), Value::Array(arr_values));
        }
    }

    // 转换为 PermissionConfig
    let json_string = serde_json::to_string(&Value::Object(json_value))?;
    let config: PermissionConfig = serde_json::from_str(&json_string)?;

    Ok(config)
}

/// 解析 TOML 键值对 `key = value`
fn parse_kv_pair(line: &str) -> Option<(String, Value)> {
    // 移除行内注释
    let without_comment = if let Some(pos) = line.find('#') {
        // 检查 # 是否在引号内
        let before = &line[..pos];
        let quote_count = before.matches('"').count();
        if quote_count % 2 == 0 {
            before
        } else {
            line
        }
    } else {
        line
    };

    let eq_pos = without_comment.find('=')?;
    let key = without_comment[..eq_pos].trim().to_string();
    let raw_value = without_comment[eq_pos + 1..].trim();

    if key.is_empty() || raw_value.is_empty() {
        return None;
    }

    let value = parse_toml_value(raw_value);
    Some((key, value))
}

/// 解析 TOML 值
fn parse_toml_value(raw: &str) -> Value {
    let raw = raw.trim();

    // 字符串（双引号或单引号）
    if (raw.starts_with('"') && raw.ends_with('"'))
        || (raw.starts_with('\'') && raw.ends_with('\''))
    {
        let inner = &raw[1..raw.len() - 1];
        return Value::String(inner.to_string());
    }

    // 布尔值
    if raw == "true" {
        return Value::Bool(true);
    }
    if raw == "false" {
        return Value::Bool(false);
    }

    // 整数
    if let Ok(n) = raw.parse::<i64>() {
        return Value::Number(serde_json::Number::from(n));
    }

    // 回退：作为字符串处理
    Value::String(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_load() {
        let config = PermissionConfigLoader::load("/test/project");
        assert_eq!(config.project_root, "/test/project");
        assert!(!config.path_rules.is_empty());
    }

    #[test]
    fn test_simple_toml_parse_kv() {
        let toml = r#"
version = "1.0"
project_root = "/test/path"
"#;
        let config = basic_toml_parse(toml).unwrap();
        assert_eq!(config.version, "1.0");
        assert_eq!(config.project_root, "/test/path");
    }

    #[test]
    fn test_toml_parse_array() {
        let toml = r#"
version = "1.0"
project_root = "/test"

[[command_rules]]
action = "Deny"
pattern = "sudo"
description = "禁止 sudo"
"#;
        let config = basic_toml_parse(toml).unwrap();
        assert_eq!(config.command_rules.len(), 1);
        assert_eq!(config.command_rules[0].pattern, "sudo");
    }

    #[test]
    fn test_toml_parse_bool() {
        let toml = r#"
version = "1.0"
project_root = "/test"

[[sensitive_file_rules]]
action = "Deny"
pattern = "**/.env"
read_allowed = false
write_allowed = false
description = "env"
"#;
        let config = basic_toml_parse(toml).unwrap();
        assert_eq!(config.sensitive_file_rules.len(), 1);
        assert!(!config.sensitive_file_rules[0].read_allowed);
        assert!(!config.sensitive_file_rules[0].write_allowed);
    }

    #[test]
    fn test_toml_with_comments() {
        let toml = r#"
# 这是注释
version = "1.0"
project_root = "/test" # 行内注释
"#;
        let config = basic_toml_parse(toml).unwrap();
        assert_eq!(config.version, "1.0");
    }
}
