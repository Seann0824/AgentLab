// src/dag/pipeline.rs
// Pipeline 定义 — 一个完整的 DAG 任务定义

use crate::dag::edge::EdgeDef;
use crate::dag::node::NodeDef;
use crate::dag::types::DAGResult;

/// Pipeline 全局配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineConfig {
    /// 最大并行节点数
    pub max_concurrency: usize,
    /// 节点超时秒数
    pub node_timeout_seconds: u64,
    /// 审核最大重试次数
    pub max_review_retries: u32,
    /// 是否在审核失败时跳过节点（标记为 skipped）
    pub skip_on_review_fail: bool,
    /// 工作 Agent 的模型（不指定则使用全局模型）
    pub worker_model: Option<String>,
    /// 审核 Agent 的模型（不指定则使用全局模型）
    pub reviewer_model: Option<String>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 2,
            node_timeout_seconds: 300,
            max_review_retries: 3,
            skip_on_review_fail: false,
            worker_model: None,
            reviewer_model: None,
        }
    }
}

/// Pipeline — 一个完整的 DAG 任务定义
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineDef {
    /// 唯一标识
    pub id: String,
    /// 描述
    pub description: String,
    /// 所有节点定义
    pub nodes: Vec<NodeDef>,
    /// 所有边定义（依赖关系）
    pub edges: Vec<EdgeDef>,
    /// DAG 全局配置
    pub config: PipelineConfig,
}

impl PipelineDef {
    /// 创建新的 Pipeline 定义
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
            config: PipelineConfig::default(),
        }
    }

    /// 添加一个节点
    pub fn add_node(mut self, node: NodeDef) -> Self {
        // 检查是否已存在相同 ID 的节点
        if !self.nodes.iter().any(|n| n.id == node.id) {
            self.nodes.push(node);
        }
        self
    }

    /// 添加一条边
    pub fn add_edge(mut self, edge: EdgeDef) -> Self {
        self.edges.push(edge);
        self
    }

    /// 设置 Pipeline 配置
    pub fn config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    /// 获取节点 ID 列表
    pub fn node_ids(&self) -> Vec<String> {
        self.nodes.iter().map(|n| n.id.clone()).collect()
    }

    /// 通过 ID 查找节点定义
    pub fn find_node(&self, id: &str) -> Option<&NodeDef> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 获取节点的上游节点 ID 列表
    pub fn upstream_nodes(&self, node_id: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|e| e.to == node_id)
            .map(|e| e.from.clone())
            .collect()
    }

    /// 获取节点的下游节点 ID 列表
    pub fn downstream_nodes(&self, node_id: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|e| e.from == node_id)
            .map(|e| e.to.clone())
            .collect()
    }

    /// 验证 Pipeline 定义的有效性
    pub fn validate(&self) -> DAGResult<()> {
        use crate::dag::types::DAGError;

        // 1. 检查是否至少有一个节点
        if self.nodes.is_empty() {
            return Err(DAGError::Internal("Pipeline 必须包含至少一个节点".to_string()));
        }

        // 2. 检查边的节点是否存在
        for edge in &self.edges {
            if !self.nodes.iter().any(|n| n.id == edge.from) {
                return Err(DAGError::EdgeNodeNotFound(edge.from.clone(), edge.to.clone()));
            }
            if !self.nodes.iter().any(|n| n.id == edge.to) {
                return Err(DAGError::EdgeNodeNotFound(edge.from.clone(), edge.to.clone()));
            }
        }

        // 3. 执行拓扑排序检测环
        let node_ids = self.node_ids();
        let _ = crate::dag::edge::topological_sort(&node_ids, &self.edges)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::node::NodeDef;
    use crate::dag::edge::EdgeDef;

    #[test]
    fn test_pipeline_def_builder() {
        let pipeline = PipelineDef::new("test", "测试 Pipeline")
            .add_node(NodeDef::new("a", "节点 A"))
            .add_node(NodeDef::new("b", "节点 B"))
            .add_edge(EdgeDef::new("a", "b"));

        assert_eq!(pipeline.id, "test");
        assert_eq!(pipeline.nodes.len(), 2);
        assert_eq!(pipeline.edges.len(), 1);
    }

    #[test]
    fn test_pipeline_validate_ok() {
        let pipeline = PipelineDef::new("valid", "有效 Pipeline")
            .add_node(NodeDef::new("a", "节点 A"))
            .add_node(NodeDef::new("b", "节点 B"))
            .add_edge(EdgeDef::new("a", "b"));

        assert!(pipeline.validate().is_ok());
    }

    #[test]
    fn test_pipeline_validate_cycle() {
        let pipeline = PipelineDef::new("cycle", "有环 Pipeline")
            .add_node(NodeDef::new("a", "节点 A"))
            .add_node(NodeDef::new("b", "节点 B"))
            .add_node(NodeDef::new("c", "节点 C"))
            .add_edge(EdgeDef::new("a", "b"))
            .add_edge(EdgeDef::new("b", "c"))
            .add_edge(EdgeDef::new("c", "a")); // 环

        assert!(pipeline.validate().is_err());
    }

    #[test]
    fn test_pipeline_validate_node_not_found() {
        let pipeline = PipelineDef::new("bad-edge", "边指向不存在的节点")
            .add_node(NodeDef::new("a", "节点 A"))
            .add_edge(EdgeDef::new("a", "undefined"));

        assert!(pipeline.validate().is_err());
    }

    #[test]
    fn test_pipeline_upstream_downstream() {
        let pipeline = PipelineDef::new("graph", "图关系测试")
            .add_node(NodeDef::new("a", "节点 A"))
            .add_node(NodeDef::new("b", "节点 B"))
            .add_node(NodeDef::new("c", "节点 C"))
            .add_edge(EdgeDef::new("a", "b"))
            .add_edge(EdgeDef::new("a", "c"));

        let upstream_b = pipeline.upstream_nodes("b");
        assert_eq!(upstream_b, vec!["a"]);

        let downstream_a = pipeline.downstream_nodes("a");
        assert_eq!(downstream_a.len(), 2);
        assert!(downstream_a.contains(&"b".to_string()));
        assert!(downstream_a.contains(&"c".to_string()));
    }

    #[test]
    fn test_pipeline_find_node() {
        let pipeline = PipelineDef::new("find", "查找测试")
            .add_node(NodeDef::new("x", "节点 X").description("测试节点"));

        let node = pipeline.find_node("x").unwrap();
        assert_eq!(node.name, "节点 X");
        assert_eq!(node.description, "测试节点");

        assert!(pipeline.find_node("y").is_none());
    }

    #[test]
    fn test_pipeline_empty_nodes() {
        let pipeline = PipelineDef::new("empty", "空 Pipeline");
        assert!(pipeline.validate().is_err());
    }
}
