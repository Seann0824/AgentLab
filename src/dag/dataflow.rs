// src/dag/dataflow.rs
// 数据流管理 — 节点间的输入/输出传递与合并

use std::collections::HashMap;

use crate::dag::types::MergeStrategy;

/// 数据流管理器
#[derive(Debug, Clone)]
pub struct DataFlowManager;

impl DataFlowManager {
    /// 合并多个上游的输出
    ///
    /// # 参数
    /// * `upstream_outputs` — 上游节点 ID 到其 final_output 的映射
    /// * `strategy` — 合并策略
    pub fn merge_inputs(
        &self,
        upstream_outputs: HashMap<String, serde_json::Value>,
        strategy: &MergeStrategy,
    ) -> serde_json::Value {
        match strategy {
            MergeStrategy::ByNodeId => {
                serde_json::json!(upstream_outputs)
            }
            MergeStrategy::Array => {
                serde_json::Value::Array(
                    upstream_outputs.into_values().collect()
                )
            }
            MergeStrategy::Custom { .. } => {
                // 预留：使用脚本进行自定义合并
                serde_json::json!(upstream_outputs)
            }
        }
    }

    /// 根据输入模式提取数据
    ///
    /// # 参数
    /// * `input` — 原始输入数据
    /// * `fields` — 要提取的字段列表
    pub fn select_fields(
        &self,
        input: &serde_json::Value,
        fields: &[String],
    ) -> serde_json::Value {
        let mut result = serde_json::Map::new();
        if let Some(obj) = input.as_object() {
            for field in fields {
                if let Some(value) = obj.get(field) {
                    result.insert(field.clone(), value.clone());
                }
            }
        }
        serde_json::Value::Object(result)
    }

    /// 将文本输出转换为最终输出格式
    pub fn text_to_output(&self, text: &str) -> serde_json::Value {
        serde_json::json!({ "content": text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::types::MergeStrategy;

    #[test]
    fn test_merge_by_node_id() {
        let mut upstream = HashMap::new();
        upstream.insert("fetch".to_string(), serde_json::json!({"data": [1, 2, 3]}));
        upstream.insert("transform".to_string(), serde_json::json!({"result": "ok"}));

        let manager = DataFlowManager;
        let merged = manager.merge_inputs(upstream, &MergeStrategy::ByNodeId);

        assert_eq!(merged["fetch"]["data"], serde_json::json!([1, 2, 3]));
        assert_eq!(merged["transform"]["result"], "ok");
    }

    #[test]
    fn test_merge_array() {
        let mut upstream = HashMap::new();
        upstream.insert("a".to_string(), serde_json::json!(1));
        upstream.insert("b".to_string(), serde_json::json!(2));

        let manager = DataFlowManager;
        let merged = manager.merge_inputs(upstream, &MergeStrategy::Array);

        assert!(merged.is_array());
        assert_eq!(merged.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_select_fields() {
        let input = serde_json::json!({
            "name": "test",
            "value": 42,
            "extra": "ignored"
        });

        let manager = DataFlowManager;
        let selected = manager.select_fields(&input, &["name".to_string(), "value".to_string()]);

        assert_eq!(selected["name"], "test");
        assert_eq!(selected["value"], 42);
        assert!(selected.get("extra").is_none());
    }

    #[test]
    fn test_text_to_output() {
        let manager = DataFlowManager;
        let output = manager.text_to_output("Hello, DAG!");
        assert_eq!(output["content"], "Hello, DAG!");
    }

    #[test]
    fn test_merge_empty() {
        let upstream = HashMap::new();
        let manager = DataFlowManager;

        let merged = manager.merge_inputs(upstream.clone(), &MergeStrategy::ByNodeId);
        assert!(merged.as_object().unwrap().is_empty());

        let merged = manager.merge_inputs(upstream, &MergeStrategy::Array);
        assert!(merged.as_array().unwrap().is_empty());
    }
}
