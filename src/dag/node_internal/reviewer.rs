// src/dag/node_internal/reviewer.rs
// Reviewer Agent 封装 — 审核 Worker 的输出

use crate::dag::runtime::{call_llm, DAGContext};
use crate::dag::types::{
    CheckResult, DAGResult, ReviewCriteria, ReviewMode, ReviewOutput, WorkerOutput,
};
use crate::model::ChatMessage;

/// Reviewer Agent 配置
pub struct ReviewerConfig {
    /// Agent 名称
    pub name: String,
    /// 审核标准
    pub criteria: ReviewCriteria,
    /// Worker 的原始输出
    pub worker_output: WorkerOutput,
    /// 原始输入（用于上下文对照）
    pub original_input: serde_json::Value,
    /// 审核模式
    pub mode: ReviewMode,
}

/// 审核系统提示模板
const REVIEW_SYSTEM_PROMPT: &str = r#"你是 DAG Pipeline 中的 Reviewer Agent。
你的职责是严格审核 Worker 的输出质量。

请根据以下标准逐项检查 Worker 的输出，然后给出总体评估。

## 审核清单
{check_items}

## 审核指南
{guidelines}

## Worker 的原始输入
{input}

## Worker 的输出
{worker_output}

请按以下 JSON 格式输出审核结果（不要包含其他内容）：
```json
{{
  "passed": true/false,
  "score": 0.0-1.0,
  "feedback": "总体评价和改进建议",
  "check_results": [
    {{
      "item": "检查项描述",
      "passed": true/false,
      "comment": "对该项的评论"
    }}
  ],
  "suggestions": ["改进建议1", "改进建议2"]
}}
```"#;

/// Reviewer Agent 封装
pub struct ReviewerAgent;

impl ReviewerAgent {
    /// 执行审核
    ///
    /// 1. 构建审核提示（criteria + worker output）
    /// 2. 调用 LLM
    /// 3. 解析 JSON 格式的审核结果
    /// 4. 返回 ReviewOutput
    pub async fn review(
        ctx: &DAGContext,
        config: ReviewerConfig,
    ) -> DAGResult<ReviewOutput> {
        // 构建审核清单字符串
        let check_items = if config.criteria.check_items.is_empty() {
            String::from("无特定检查项")
        } else {
            config.criteria.check_items
                .iter()
                .enumerate()
                .map(|(i, item)| format!("{}. {}", i + 1, item))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let guidelines = if config.criteria.guidelines.is_empty() {
            String::from("无特定指南")
        } else {
            config.criteria.guidelines.clone()
        };

        let worker_output_str = if config.worker_output.structured.is_some() {
            serde_json::to_string_pretty(&config.worker_output.structured)
                .unwrap_or_default()
        } else {
            config.worker_output.content.clone()
        };

        // 构建系统提示
        let system_prompt = REVIEW_SYSTEM_PROMPT
            .replace("{check_items}", &check_items)
            .replace("{guidelines}", &guidelines)
            .replace(
                "{input}",
                &serde_json::to_string_pretty(&config.original_input).unwrap_or_default()
            )
            .replace("{worker_output}", &worker_output_str);

        let messages = vec![
            ChatMessage::system(&system_prompt),
            ChatMessage::user("请审核上述 Worker 输出并返回 JSON 格式的审核结果。"),
        ];

        // 调用 LLM
        let (response, _) = call_llm(&ctx.model, messages, serde_json::json!([])).await?;

        // 从响应中提取 JSON
        let json_str = extract_json(&response).unwrap_or_else(|| &response);

        // 解析 JSON
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
            let passed = json["passed"].as_bool().unwrap_or(false);
            let score = json["score"].as_f64().unwrap_or(0.0) as f32;
            let feedback = json["feedback"].as_str().unwrap_or("").to_string();

            let check_results = json["check_results"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|item| CheckResult {
                            item: item["item"].as_str().unwrap_or("").to_string(),
                            passed: item["passed"].as_bool().unwrap_or(false),
                            comment: item["comment"].as_str().unwrap_or("").to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();

            let suggestions = json["suggestions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            Ok(ReviewOutput {
                passed,
                score,
                feedback,
                check_results,
                suggestions,
            })
        } else {
            // 回退：JSON 解析失败，判定为不通过
            Ok(ReviewOutput {
                passed: false,
                score: 0.0,
                feedback: format!("审核结果无法解析为 JSON，判定不通过。原始响应: {}", response),
                check_results: vec![],
                suggestions: vec![],
            })
        }
    }
}

/// 从响应文本中提取 JSON（去掉 markdown 代码块标记）
fn extract_json<'a>(text: &'a str) -> Option<&'a str> {
    // 尝试去掉 ```json ... ``` 包裹
    if let Some(start) = text.find("```json") {
        let content_start = start + 7;
        if let Some(end) = text[content_start..].find("```") {
            return Some(text[content_start..content_start + end].trim());
        }
    }
    // 尝试去掉 ``` ... ``` 包裹
    if let Some(start) = text.find("```") {
        let content_start = start + 3;
        if let Some(end) = text[content_start..].find("```") {
            return Some(text[content_start..content_start + end].trim());
        }
    }
    // 尝试直接解析为 JSON
    let trimmed = text.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }
    None
}
