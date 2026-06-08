# 功能特性设计：上下文窗口管理 (Context Window Management) — 实现计划与测试策略

> 原文拆分自 `../context-window.md`。

## 4. 实现计划

```
Phase 1: Token 估算器（0.25天）
├── 实现 TokenEstimator（基于字符统计的经验公式）
├── 实现增量估算（每次只估新增消息，缓存总计数）
├── 单元测试：英文、中文、代码、混合文本
└── 校准脚本：scripts/calibrate_tokenizer.py

Phase 2: 消息重要性标记系统（0.25天）
├── 实现 ContextMessage + MessageImportance
├── 实现自动分类（auto_classify）
├── 实现 preserve 标记接口
└── 单元测试：重要性分类正确性

Phase 3: 三层压缩策略（0.5天）
├── 实现滑动窗口（preserve 消息保护逻辑）
├── 实现自动模式（三层触发逻辑）
├── 实现保底截断
├── 实现对话轮数计数器（考虑 preserve 消息）
└── 单元测试：边界条件、保留正确性

Phase 4: 异步摘要生成器（0.25天）
├── 实现 AsyncSummarizer（channel + background task）
├── 实现结构化 LLM 摘要提示词
├── 实现规则摘要兜底
└── 单元测试：摘要格式、异步注入

Phase 5: ContextManager + 系统集成（0.25天）
├── 实现 ContextManager（增量 Token + 自动压缩触发）
├── 实现统计信息收集 (ContextStats + preserved_count)
├── 修改 main.rs 使用 ContextManager
├── stdout/stderr 分离
├── 系统提示词补充
└── 集成测试：长时间对话不崩溃

总计: 1.5 天
```

---

## 5. 测试策略

### 5.1 单元测试

| 测试用例 | 目标 |
|---------|------|
| `test_estimate_short_text` | 短文本 Token 估算 |
| `test_estimate_long_code` | 代码文本 Token 估算 |
| `test_estimate_message_types` | 各消息类型 Token 估算 |
| `test_incremental_token_tracking` | 增量估算与全量估算结果一致 |
| `test_sliding_window_basic` | 基本滑动窗口功能 |
| `test_sliding_window_protects_system` | 系统提示词不被删除 |
| `test_sliding_window_protects_preserved` | ⭐ 标记为 preserve 的消息不被删除 |
| `test_sliding_window_milestone_priority` | ⭐ Milestone 消息优先保留 |
| `test_sliding_window_exact_limit` | 恰好等于窗口大小的边界 |
| `test_sliding_window_below_limit` | 低于窗口大小不触发 |
| `test_auto_no_compression_needed` | 低 Token 使用不触发 |
| `test_auto_sliding_window_first` | ⭐ 自动模式优先滑动窗口 |
| `test_auto_hard_truncate` | ⭐ 极端情况下保底截断 |
| `test_context_manager_add_and_compress` | 完整添加+压缩流程 |
| `test_context_manager_stats` | 统计信息正确性（含 preserved_count） |
| `test_importance_classification` | ⭐ 自动分类正确性 |
| `test_rule_based_summary_structure` | ⭐ 规则摘要结构化输出 |

### 5.2 集成测试

```rust
#[tokio::test]
async fn test_long_conversation_with_preserved_messages() {
    let mut ctx = ContextManager::new(
        "system prompt".to_string(),
        ContextStrategy::Auto {
            token_limit: 10_000,    // 小限制方便测试
            max_turns: 3,
            trigger_ratio: 0.5,
            enable_async_summary: false,  // 测试中关闭异步摘要
        },
    );

    // 模拟 50 轮对话，其中一些标记为重要
    for i in 0..50 {
        ctx.add_message(ChatMessage::user(format!("user message {}", i)));

        // 模拟工具调用
        ctx.add_message(ChatMessage::assistant_tool_calls(
            format!("thinking {}", i),
            vec![ToolCall { id: format!("call_{}", i), name: "shell".into(), arguments: r#"{"command": "echo ok"}"#.into() }],
        ));

        let tool_result = ChatMessage::tool(
            format!("call_{}", i),
            r#"{"ok": true, "result": {"stdout": "ok\n"}}"#.into(),
        );
        ctx.add_message(tool_result);

        ctx.add_message(ChatMessage::assistant(format!("response {}", i)));

        // 第 10 轮标记为重要（模拟读取了关键文件）
        if i == 10 {
            ctx.preserve_last_message();
        }

        // 验证系统提示词始终存在
        assert!(ctx.get_messages().iter().any(|m| matches!(m, ChatMessage::System { .. })));
    }

    // 验证系统提示词还在
    assert!(ctx.get_messages().iter().any(|m| matches!(m, ChatMessage::System { .. })));

    // 验证压缩后的消息数不会无限增长
    assert!(ctx.get_messages().len() < 50, "消息数应被压缩控制");

    // ⭐ 验证被 preserve 的消息没有被丢弃
    let stats = ctx.stats();
    assert!(stats.preserved_count > 0, "应有被保留的重要消息");
    assert!(stats.compressed, "应已触发压缩");
}
```

### 5.3 精度验证测试

```rust
/// ⭐ Token 估算精度交叉验证
///
/// 需要 tiktoken-rs 作为 dev-dependency 运行
#[cfg(feature = "calibration")]
#[tokio::test]
async fn test_token_estimator_accuracy() {
    use tiktoken_rs::cl100k_base;

    let estimator = TokenEstimator::new();
    let bpe = cl100k_base().unwrap();

    // 从 fixtures 加载真实对话样本
    let samples = load_test_fixtures("tests/fixtures/real_conversations.json");

    let mut total_estimated = 0;
    let mut total_actual = 0;

    for sample in &samples {
        let estimated = estimator.estimate_text(sample);
        let actual = bpe.encode_with_special_tokens(sample).len();
        total_estimated += estimated;
        total_actual += actual;
    }

    let error = (total_estimated as f64 - total_actual as f64) / total_actual as f64;
    let error_pct = error * 100.0;

    println!("Total estimated: {}", total_estimated);
    println!("Total actual:    {}", total_actual);
    println!("Error:           {:.1}%", error_pct);

    // 允许 ±20% 误差
    assert!(
        error.abs() < 0.20,
        "Token 估算误差 {:.1}% 超出 ±20% 允许范围",
        error_pct
    );

    // 计算校准系数
    let calibration_factor = total_actual as f64 / total_estimated as f64;
    println!("Recommended calibration_factor: {:.3}", calibration_factor);
}
```

---

