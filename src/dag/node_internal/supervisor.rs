// src/dag/node_internal/supervisor.rs
// NodeSupervisor — 节点内部协调器
//
// 管理节点内部 Worker + Reviewer 的生命周期，包括重试逻辑。
// 工作流程: input → Worker → Reviewer → (通过→输出 / 不通过→Worker 修订→Reviewer 再审...)

use crate::dag::node::NodeDef;
use crate::dag::node_internal::reviewer::{ReviewerAgent, ReviewerConfig};
use crate::dag::node_internal::worker::{WorkerAgent, WorkerConfig};
use crate::dag::runtime::DAGContext;
use crate::dag::types::{DAGResult, NodeResult, ReviewOutput, WorkerOutput};

/// 节点内部协调器
pub struct NodeSupervisor;

impl NodeSupervisor {
    /// 构建审核反馈文本（用于重试时注入 Worker 提示）
    fn build_feedback_text(review: &ReviewOutput) -> String {
        let mut parts = Vec::new();

        parts.push(format!("评分: {:.1}/1.0", review.score));
        parts.push(format!("反馈: {}", review.feedback));

        if !review.check_results.is_empty() {
            parts.push("\n逐项检查结果:".to_string());
            for cr in &review.check_results {
                let status = if cr.passed { "✅" } else { "❌" };
                parts.push(format!("  {} {} - {}", status, cr.item, cr.comment));
            }
        }

        if !review.suggestions.is_empty() {
            parts.push("\n改进建议:".to_string());
            for s in &review.suggestions {
                parts.push(format!("  - {}", s));
            }
        }

        parts.join("\n")
    }

    /// 执行节点（单次 Worker → Reviewer）
    ///
    /// 返回 NodeResult：
    /// - Success: 审核通过，包含输出
    /// - NeedsRevision: 审核不通过，包含 Worker 输出和审核反馈
    pub async fn execute(
        ctx: &DAGContext,
        node_def: &NodeDef,
        input: serde_json::Value,
        previous_feedback: Option<String>,
    ) -> DAGResult<NodeResult> {
        // Step 1: Worker
        let worker_config = WorkerConfig {
            name: node_def.name.clone(),
            instruction: node_def.worker_instruction.clone(),
            input: input.clone(),
            max_turns: 5,
            previous_feedback,
        };
        let worker_output = WorkerAgent::execute(ctx, worker_config).await?;

        // Step 2: Reviewer
        let reviewer_config = ReviewerConfig {
            name: format!("{}-reviewer", node_def.name),
            criteria: node_def.review_criteria.clone(),
            worker_output: worker_output.clone(),
            original_input: input,
            mode: crate::dag::types::ReviewMode::Checklist,
        };
        let review = ReviewerAgent::review(ctx, reviewer_config).await?;

        // Step 3: 判断审核结果
        if review.passed {
            Ok(NodeResult::Success {
                output: worker_output.content,
                review,
            })
        } else {
            Ok(NodeResult::NeedsRevision {
                worker_output,
                review,
            })
        }
    }

    /// 带重试的执行
    ///
    /// 内部循环：Worker → Reviewer → (通过→输出 / 不通过→注入反馈→重试)
    /// 达到 max_retries 后返回 FailedAfterRetries
    pub async fn execute_with_retry(
        ctx: &DAGContext,
        node_def: &NodeDef,
        input: serde_json::Value,
        max_retries: u32,
    ) -> DAGResult<NodeResult> {
        let mut last_worker_output: Option<WorkerOutput> = None;
        let mut last_review = None;
        let mut retries = 0u32;
        // 累积的审核反馈链（历史反馈 + 本次反馈）
        let mut feedback_chain: Option<String> = None;

        loop {
            let result = Self::execute(ctx, node_def, input.clone(), feedback_chain.clone()).await?;

            match result {
                NodeResult::Success { .. } => {
                    return Ok(result);
                }
                NodeResult::NeedsRevision { worker_output, review } => {
                    last_worker_output = Some(worker_output);
                    let feedback_text = Self::build_feedback_text(&review);
                    last_review = Some(review.clone());

                    // 构建累积反馈链（包含历史反馈 + 本次反馈）
                    feedback_chain = match feedback_chain {
                        Some(mut chain) => {
                            chain.push_str(&format!("\n---\n### 第 {} 次修订\n{}", retries + 1, feedback_text));
                            Some(chain)
                        }
                        None => Some(format!("### 第 {} 次修订\n{}", retries + 1, feedback_text)),
                    };

                    retries += 1;
                    if retries > max_retries {
                        return Ok(NodeResult::FailedAfterRetries {
                            last_worker_output: last_worker_output.unwrap(),
                            last_review: last_review.unwrap(),
                            retries,
                        });
                    }
                    // 继续循环重试（带反馈链）
                }
                NodeResult::FailedAfterRetries { .. } => {
                    return Ok(result);
                }
            }
        }
    }
}
