use std::collections::HashMap;

use openai_api_rs::v1::types;
use serde::Deserialize;
use serde_json::Value;

use crate::agent::tool_agent::ToolAgent;
use crate::base::llm::AgentsLLM;
use crate::tools::memory::base::{
    ConflictResolution, ExistingAction, ExistingMemoryDecision, MemoryItem, NewFactAction,
};
use crate::tools::types::{Tool, ToolError};
use crate::tools::ToolManager;

/// 一条待裁决的新事实及其候选记忆。
#[derive(Clone)]
pub struct ConflictCheckRequest {
    pub fact_index: usize,
    pub fact_content: String,
    pub candidates: Vec<MemoryItem>,
}

/// 内部子 agent：批量判断若干新事实与候选记忆之间的冲突关系，
/// 并决定是新增、跳过，还是更新/失效已有记忆。
///
/// 采用单工具调用返回批量结果，参考 `EntityExtractorAgent` 的实现方式。
pub struct MemoryConflictResolver {
    inner: ToolAgent<BatchResolutionResult>,
}

impl MemoryConflictResolver {
    pub fn new(llm: AgentsLLM) -> Self {
        let system_prompt = r#"You are a Memory Conflict Resolver. You will receive multiple newly extracted facts, each with a list of existing memory candidates. For each fact, decide how to reconcile it with its candidates.

Rules:
1. DUPLICATE: If the new fact is semantically equivalent to an existing memory (same meaning, possibly rephrased), the new fact should be SKIPPED and the existing memory KEPT.
2. UPDATE: If the new fact adds details or clarifies an existing memory without contradicting it (e.g., "I have a cat" + "My cat is named Mimi"), merge them by UPDATING the existing memory, and SKIP the new fact.
3. INVALIDATE: If the new fact contradicts or overrides an existing memory (e.g., "My English name is Sean" vs "I changed my name to Beta"), mark the existing memory as INVALIDATED and ADD the new fact.
4. INDEPENDENT: If the new fact is related but not conflicting (different time, different aspect), KEEP the existing memory and ADD the new fact.
5. Only INVALIDATE when there is a clear contradiction or replacement. When in doubt, prefer KEEP + ADD.
6. Provide a brief reason for each decision.

Output must call the resolve_conflicts_batch tool with a resolutions array, one entry per fact_index. If a fact has no candidates, return new_fact_action="add" and an empty existing_memories array."#;

        let mut tool_manager = ToolManager::new();
        tool_manager.register_tool(Box::new(ResolveConflictsBatchTool));

        let inner = ToolAgent::new("memory_conflict_resolver", llm, system_prompt, tool_manager);

        Self { inner }
    }

    /// 对单条新事实与候选记忆进行冲突裁决。
    ///
    /// 内部走批量接口，保持旧接口兼容。
    pub async fn resolve(
        &mut self,
        new_fact: &str,
        candidates: &[MemoryItem],
    ) -> Result<ConflictResolution, String> {
        let requests = vec![ConflictCheckRequest {
            fact_index: 0,
            fact_content: new_fact.to_string(),
            candidates: candidates.to_vec(),
        }];
        let results = self.resolve_batch(requests).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| "[MemoryConflictResolver] batch returned empty results".to_string())
    }

    /// 批量冲突裁决。
    ///
    /// `requests` 中每个元素包含事实索引、事实内容、该事实召回的候选记忆。
    /// 返回与输入顺序对应的 `ConflictResolution` 列表。
    pub async fn resolve_batch(
        &mut self,
        requests: Vec<ConflictCheckRequest>,
    ) -> Result<Vec<ConflictResolution>, String> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        // 没有任何候选的事实直接判为新增，无需 LLM。
        let mut fast_results: HashMap<usize, ConflictResolution> = HashMap::new();
        let mut llm_requests = Vec::new();
        for req in requests {
            if req.candidates.is_empty() {
                fast_results.insert(req.fact_index, ConflictResolution::add_new());
            } else {
                llm_requests.push(req);
            }
        }

        let original_count = fast_results.len() + llm_requests.len();

        if llm_requests.is_empty() {
            return Ok(restore_order(original_count, &fast_results));
        }

        let input = build_batch_resolution_prompt(&llm_requests);
        let result = self.inner.run(&input).await?;
        let mut result_map: HashMap<usize, ConflictResolution> = result
            .resolutions
            .into_iter()
            .map(|r| (r.fact_index, r.into_conflict_resolution()))
            .collect();

        // 合并快速结果。
        result_map.extend(fast_results);

        Ok(restore_order(original_count, &result_map))
    }
}

fn restore_order(
    count: usize,
    map: &HashMap<usize, ConflictResolution>,
) -> Vec<ConflictResolution> {
    (0..count)
        .map(|idx| {
            map.get(&idx)
                .cloned()
                .unwrap_or_else(ConflictResolution::add_new)
        })
        .collect()
}

fn build_batch_resolution_prompt(requests: &[ConflictCheckRequest]) -> String {
    let mut lines = vec!["New facts to store and their candidate memories:".to_string()];

    for req in requests {
        lines.push(format!("\n[Fact {}] {}", req.fact_index, req.fact_content));
        if req.candidates.is_empty() {
            lines.push("  (no candidates)".to_string());
        } else {
            for (i, item) in req.candidates.iter().enumerate() {
                lines.push(format!(
                    "  [Candidate {}] id={} type={} timestamp={} importance={:.2}\n      content: {}",
                    i + 1,
                    item.id,
                    item.memory_type,
                    item.timestamp,
                    item.importance,
                    item.content
                ));
            }
        }
    }

    lines.push("".to_string());
    lines.push(
        "Please call the resolve_conflicts_batch tool and provide a resolution for EACH fact_index listed above."
            .to_string(),
    );

    lines.join("\n")
}

/// ToolAgent 反序列化用的中间结构（批量）。
#[derive(Deserialize)]
struct BatchResolutionResult {
    resolutions: Vec<ResolutionResult>,
}

#[derive(Deserialize)]
struct ResolutionResult {
    fact_index: usize,
    new_fact_action: String,
    merged_into: Option<String>,
    existing_memories: Vec<ExistingMemoryResult>,
}

#[derive(Deserialize)]
struct ExistingMemoryResult {
    memory_id: String,
    action: String,
    merged_content: Option<String>,
    reason: String,
}

impl ResolutionResult {
    fn into_conflict_resolution(self) -> ConflictResolution {
        let new_fact_action = match self.new_fact_action.as_str() {
            "skip" | "Skip" | "SKIP" => NewFactAction::Skip {
                merged_into: self.merged_into,
            },
            _ => NewFactAction::Add,
        };

        let existing_memories = self
            .existing_memories
            .into_iter()
            .map(|r| ExistingMemoryDecision {
                memory_id: r.memory_id,
                action: parse_existing_action(&r.action),
                merged_content: r.merged_content,
                reason: r.reason,
            })
            .collect();

        ConflictResolution {
            new_fact_action,
            existing_memories,
        }
    }
}

fn parse_existing_action(action: &str) -> ExistingAction {
    match action.to_lowercase().as_str() {
        "keep" => ExistingAction::Keep,
        "update" => ExistingAction::Update,
        "invalidate" => ExistingAction::Invalidate,
        "delete" => ExistingAction::Delete,
        _ => ExistingAction::Keep,
    }
}

/// 子 agent 唯一拥有的工具：只提供 schema，实际不执行任何操作。
///
/// 输出为批量 resolutions，每个 resolution 通过 fact_index 与输入事实对应。
struct ResolveConflictsBatchTool;

#[async_trait::async_trait]
impl Tool for ResolveConflictsBatchTool {
    fn name(&self) -> &str {
        "resolve_conflicts_batch"
    }

    fn description(&self) -> &str {
        "批量裁决新事实与已有记忆之间的冲突关系"
    }

    fn parameters_schema(&self) -> types::FunctionParameters {
        let existing_action_values = vec![
            "keep".to_string(),
            "update".to_string(),
            "invalidate".to_string(),
            "delete".to_string(),
        ];

        let existing_item = types::JSONSchemaDefine {
            schema_type: Some(types::JSONSchemaType::Object),
            properties: Some(HashMap::from([
                (
                    "memory_id".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::String),
                        description: Some("候选记忆的 ID".to_string()),
                        ..Default::default()
                    }),
                ),
                (
                    "action".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::String),
                        description: Some(
                            "对该记忆的操作: keep(保留), update(更新), invalidate(失效), delete(删除)"
                                .to_string(),
                        ),
                        enum_values: Some(existing_action_values),
                        ..Default::default()
                    }),
                ),
                (
                    "merged_content".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::String),
                        description: Some("action=update 时，合并后的新内容".to_string()),
                        ..Default::default()
                    }),
                ),
                (
                    "reason".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::String),
                        description: Some("做出该决策的简短理由".to_string()),
                        ..Default::default()
                    }),
                ),
            ])),
            required: Some(vec![
                "memory_id".to_string(),
                "action".to_string(),
                "reason".to_string(),
            ]),
            ..Default::default()
        };

        let resolution_item = types::JSONSchemaDefine {
            schema_type: Some(types::JSONSchemaType::Object),
            properties: Some(HashMap::from([
                (
                    "fact_index".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::Number),
                        description: Some("对应输入事实的索引".to_string()),
                        ..Default::default()
                    }),
                ),
                (
                    "new_fact_action".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::String),
                        description: Some(
                            "对新增事实的操作: add(新增) 或 skip(跳过，因为重复或已合并)"
                                .to_string(),
                        ),
                        enum_values: Some(vec!["add".to_string(), "skip".to_string()]),
                        ..Default::default()
                    }),
                ),
                (
                    "merged_into".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::String),
                        description: Some(
                            "new_fact_action=skip 时，指向合并到的目标记忆 ID".to_string(),
                        ),
                        ..Default::default()
                    }),
                ),
                (
                    "existing_memories".to_string(),
                    Box::new(types::JSONSchemaDefine {
                        schema_type: Some(types::JSONSchemaType::Array),
                        description: Some("对该事实的每条候选已有记忆的裁决".to_string()),
                        items: Some(Box::new(existing_item)),
                        ..Default::default()
                    }),
                ),
            ])),
            required: Some(vec![
                "fact_index".to_string(),
                "new_fact_action".to_string(),
                "existing_memories".to_string(),
            ]),
            ..Default::default()
        };

        types::FunctionParameters {
            schema_type: types::JSONSchemaType::Object,
            properties: Some(HashMap::from([(
                "resolutions".to_string(),
                Box::new(types::JSONSchemaDefine {
                    schema_type: Some(types::JSONSchemaType::Array),
                    description: Some("每条输入事实的裁决结果".to_string()),
                    items: Some(Box::new(resolution_item)),
                    ..Default::default()
                }),
            )])),
            required: Some(vec!["resolutions".to_string()]),
        }
    }

    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        serde_json::to_string(&args).map_err(|e| {
            ToolError::Internal(format!(
                "[ResolveConflictsBatchTool] serialize args failed: {}",
                e
            ))
        })
    }
}
