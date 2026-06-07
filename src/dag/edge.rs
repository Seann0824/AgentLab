// src/dag/edge.rs
// 边定义 — 节点间的依赖关系和数据映射

use crate::dag::types::DAGResult;

/// 数据映射规则
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DataMapping {
    /// 从源输出的字段提取
    pub source_fields: Vec<String>,
    /// 映射到目标输入的字段名
    pub target_fields: Vec<String>,
    /// 数据转换表达式（可选，预留）
    pub transform: Option<String>,
}

impl DataMapping {
    pub fn new(source_fields: Vec<String>, target_fields: Vec<String>) -> Self {
        Self {
            source_fields,
            target_fields,
            transform: None,
        }
    }
}

/// 边的定义 — 节点间的依赖和数据流向
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EdgeDef {
    /// 来源节点 ID
    pub from: String,
    /// 目标节点 ID
    pub to: String,
    /// 数据映射规则（可选）
    pub data_mapping: Option<DataMapping>,
}

impl EdgeDef {
    /// 创建一个简单的依赖边（无数据映射）
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            data_mapping: None,
        }
    }

    /// 创建带数据映射的边
    pub fn with_mapping(
        from: impl Into<String>,
        to: impl Into<String>,
        mapping: DataMapping,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            data_mapping: Some(mapping),
        }
    }
}

// =====================================================================
// 图操作工具函数
// =====================================================================

/// 使用 Kahn 算法进行拓扑排序，检测环
///
/// 返回节点的拓扑顺序列表，如果存在环则返回错误。
///
/// # 参数
/// * `nodes` — 所有节点 ID 列表
/// * `edges` — 所有边的列表（`from` → `to`）
///
/// # 返回值
/// * `Ok(Vec<String>)` — 拓扑排序后的节点 ID 列表
/// * `Err(DAGError::CycleDetected)` — 检测到环
pub fn topological_sort(nodes: &[String], edges: &[EdgeDef]) -> DAGResult<Vec<String>> {
    use std::collections::{HashMap, VecDeque};

    // 构建邻接表和入度表
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    // 初始化所有节点的入度为 0
    for node_id in nodes {
        in_degree.entry(node_id.as_str()).or_insert(0);
        adjacency.entry(node_id.as_str()).or_default();
    }

    // 构建依赖关系
    for edge in edges {
        // 验证节点是否存在
        if !in_degree.contains_key(edge.from.as_str()) {
            return Err(crate::dag::types::DAGError::EdgeNodeNotFound(
                edge.from.clone(),
                edge.to.clone(),
            ));
        }
        if !in_degree.contains_key(edge.to.as_str()) {
            return Err(crate::dag::types::DAGError::EdgeNodeNotFound(
                edge.from.clone(),
                edge.to.clone(),
            ));
        }

        adjacency
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
        *in_degree.entry(edge.to.as_str()).or_insert(0) += 1;
    }

    // Kahn 算法
    let mut queue: VecDeque<&str> = VecDeque::new();
    for (node, &degree) in in_degree.iter() {
        if degree == 0 {
            queue.push_back(node);
        }
    }

    let mut sorted: Vec<String> = Vec::with_capacity(nodes.len());

    while let Some(node) = queue.pop_front() {
        sorted.push(node.to_string());

        if let Some(neighbors) = adjacency.get(node) {
            for neighbor in neighbors {
                if let Some(degree) = in_degree.get_mut(neighbor) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    if sorted.len() != nodes.len() {
        return Err(crate::dag::types::DAGError::CycleDetected);
    }

    Ok(sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topological_sort_simple_chain() {
        let nodes = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let edges = vec![
            EdgeDef::new("a", "b"),
            EdgeDef::new("b", "c"),
        ];

        let result = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_topological_sort_branching() {
        let nodes = vec![
            "fetch".to_string(),
            "transform".to_string(),
            "analyze".to_string(),
            "report".to_string(),
        ];
        let edges = vec![
            EdgeDef::new("fetch", "transform"),
            EdgeDef::new("transform", "analyze"),
            EdgeDef::new("transform", "report"),
            EdgeDef::new("analyze", "report"),
        ];

        let result = topological_sort(&nodes, &edges).unwrap();
        // fetch 必须在第一，transform 必须在第二
        assert_eq!(result[0], "fetch");
        assert_eq!(result[1], "transform");
        // analyze 必须在 report 之前
        let pos_analyze = result.iter().position(|x| x == "analyze").unwrap();
        let pos_report = result.iter().position(|x| x == "report").unwrap();
        assert!(pos_analyze < pos_report);
    }

    #[test]
    fn test_topological_sort_parallel() {
        let nodes = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        let edges = vec![
            EdgeDef::new("a", "b"),
            EdgeDef::new("a", "c"),
            EdgeDef::new("b", "d"),
            EdgeDef::new("c", "d"),
        ];

        let result = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(result[0], "a");
        assert_eq!(result[result.len() - 1], "d");
    }

    #[test]
    fn test_topological_sort_cycle_detected() {
        let nodes = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let edges = vec![
            EdgeDef::new("a", "b"),
            EdgeDef::new("b", "c"),
            EdgeDef::new("c", "a"), // 环！
        ];

        let result = topological_sort(&nodes, &edges);
        assert!(result.is_err());
        match result {
            Err(e) => assert!(e.to_string().contains("环")),
            _ => panic!("Expected CycleDetected error"),
        }
    }

    #[test]
    fn test_topological_sort_single_node() {
        let nodes = vec!["a".to_string()];
        let edges: Vec<EdgeDef> = vec![];

        let result = topological_sort(&nodes, &edges).unwrap();
        assert_eq!(result, vec!["a"]);
    }

    #[test]
    fn test_topological_sort_edge_node_not_found() {
        let nodes = vec!["a".to_string(), "b".to_string()];
        let edges = vec![EdgeDef::new("a", "undefined")];

        let result = topological_sort(&nodes, &edges);
        assert!(result.is_err());
    }

    #[test]
    fn test_data_mapping() {
        let mapping = DataMapping::new(
            vec!["users".to_string()],
            vec!["input_users".to_string()],
        );
        assert_eq!(mapping.source_fields, vec!["users"]);
        assert_eq!(mapping.target_fields, vec!["input_users"]);
        assert!(mapping.transform.is_none());
    }

    #[test]
    fn test_edge_with_mapping() {
        let mapping = DataMapping::new(
            vec!["result".to_string()],
            vec!["data".to_string()],
        );
        let edge = EdgeDef::with_mapping("fetch", "transform", mapping);
        assert_eq!(edge.from, "fetch");
        assert_eq!(edge.to, "transform");
        assert!(edge.data_mapping.is_some());
    }
}
