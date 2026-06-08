# Migrate to the Responses API — 常见错误、渐进发布和 Assistants API

> 原文拆分自 `../openai-response.md`。

### 9. Check common migration errors

Watch for these issues when moving code from Chat Completions to Responses:

- Reading `choices[0].message.content` instead of `response.output_text` or `response.output`.
- Treating every `output` entry as a message. Reasoning, tool, and function calls are separate Item types.
- Dropping reasoning, function call, or function call output Items when manually carrying context into the next response.
- Sending a function result without the matching `call_id`.
- Using `response_format` in a Responses request instead of `text.format`.
- Reusing Chat Completions streaming chunk handlers without handling typed Responses events.
- Assuming `previous_response_id` removes billing for prior context. Previous input tokens in the response chain are still billed as input tokens.

## Incremental rollout checklist

Chat Completions remains supported, so you can migrate one user flow at a time.

- [ ] Start with a simple text-generation flow.
- [ ] Update the endpoint, request body, and output handling.
- [ ] Decide whether the flow uses `previous_response_id`, manual Item replay, or the Conversations API.
- [ ] If the flow is stateless or ZDR, add `store: false` and include encrypted reasoning items when reasoning context must continue across turns.
- [ ] Migrate function definitions and verify function call outputs include the correct `call_id`.
- [ ] Move Structured Outputs schemas from `response_format` to `text.format`.
- [ ] Update streaming consumers to handle typed Responses events.
- [ ] Replace custom orchestration with OpenAI-hosted tools where they fit the workflow.
- [ ] Compare behavior, latency, token usage, and errors before routing more traffic to Responses.

We recommend migrating all flows to the Responses API over time to take advantage of the latest OpenAI features and improvements.

## Assistants API

Based on developer feedback from the [Assistants API](https://developers.openai.com/api/docs/api-reference/assistants) beta, we've incorporated key improvements into the Responses API to make it more flexible, faster, and easier to use. The Responses API represents the future direction for building agents on OpenAI.

We now have Assistant-like and Thread-like objects in the Responses API. Learn more in the [migration guide](https://developers.openai.com/api/docs/guides/assistants/migration). As of August 26, 2025, we're deprecating the Assistants API, with a sunset date of August 26, 2026.
