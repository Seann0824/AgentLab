# Migrate to the Responses API — Responses API 概览

> 原文拆分自 `../openai-response.md`。

## About the Responses API

The Responses API is a unified interface for building powerful, agent-like applications. It contains:

- Built-in tools like [web search](https://developers.openai.com/api/docs/guides/tools-web-search), [file search](https://developers.openai.com/api/docs/guides/tools-file-search), [computer use](https://developers.openai.com/api/docs/guides/tools-computer-use), [code interpreter](https://developers.openai.com/api/docs/guides/tools-code-interpreter), and [remote MCPs](https://developers.openai.com/api/docs/guides/tools-remote-mcp).
- Seamless multi-turn interactions that allow you to pass previous responses for higher accuracy reasoning results.
- Native multimodal support for text and images.

## Responses benefits

The Responses API contains several benefits over Chat Completions:

- **Better performance**: Using reasoning models, like GPT-5, with Responses will result in better model intelligence when compared to Chat Completions. Our internal evals reveal a 3% improvement in SWE-bench with same prompt and setup.
- **Agentic by default**: The Responses API is an agentic loop, allowing the model to call multiple tools, like `web_search`, `image_generation`, `file_search`, `code_interpreter`, remote MCP servers, as well as your own custom functions, within the span of one API request.
- **Lower costs**: Results in lower costs due to improved cache utilization (40% to 80% improvement when compared to Chat Completions in internal tests).
- **Stateful context**: Use `store: true` to maintain state from turn to turn, preserving reasoning and tool context from turn-to-turn.
- **Flexible inputs**: Pass a string with input or a list of messages; use instructions for system-level guidance.
- **Encrypted reasoning**: Opt-out of statefulness while still benefiting from advanced reasoning.
- **Future-proof**: Future-proofed for upcoming models.

<div className="roles-table">

| Capabilities        | Chat Completions API  | Responses API         |
| ------------------- | --------------------- | --------------------- |
| Text generation     | | |
| Audio               | | Coming soon           |
| Vision              | | |
| Structured Outputs  | | |
| Function calling    | | |
| Web search          | | |
| File search         | | |
| Computer use        | | |
| Code interpreter    | | |
| MCP                 | | |
| Image generation    | | |
| Reasoning summaries | | |

</div>

### Examples

See how the Responses API compares to the Chat Completions API in specific scenarios.

#### Messages vs. Items

Both APIs make it easy to generate output from our models. The input to, and result of, a call to Chat completions is an array of _Messages_, while
the Responses API uses _Items_. An Item is a union of many types, representing the range of possibilities
of model actions. A `message` is a type of Item, as is a `function_call` or `function_call_output`. Unlike a Chat Completions Message, where
many concerns are glued together into one object, Items are distinct from one another and better represent the basic unit of model context.

Additionally, Chat Completions can return multiple parallel generations as `choices`, using the `n` param. In Responses, we've removed this param, leaving only one generation.

When you get a response back from the Responses API, the fields differ slightly.
Instead of a `message`, you receive a typed `response` object with its own `id`.
Responses are stored by default. Chat completions are stored by default for new accounts.
To disable storage when using either API, set `store: false`.

The objects you receive back from these APIs will differ slightly. In Chat Completions, you receive an array of
`choices`, each containing a `message`. In Responses, you receive an array of Items labeled `output`.

### Additional differences

- Responses are stored by default. Chat completions are stored by default for new accounts. To disable storage in either API, set `store: false`.
- [Reasoning](https://developers.openai.com/api/docs/guides/reasoning) models have a richer experience in the Responses API with [improved tool usage](https://developers.openai.com/api/docs/guides/reasoning#keeping-reasoning-items-in-context). Starting with GPT-5.4, tool calling is not supported in Chat Completions with `reasoning: none`.
- Structured Outputs API shape is different. Instead of `response_format`, use `text.format` in Responses. Learn more in the [Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs) guide.
- The function-calling API shape is different, both for the function config on the request, and function calls sent back in the response. See the full difference in the [function calling guide](https://developers.openai.com/api/docs/guides/function-calling).
- The Responses SDK has an `output_text` helper, which the Chat Completions SDK does not have.
- In Chat Completions, conversation state must be managed manually. The Responses API has compatibility with the [Conversations API](https://developers.openai.com/api/docs/guides/conversation-state?api-mode=responses#using-the-conversations-api) for persistent conversations, or the ability to pass a `previous_response_id` to easily chain Responses together.

## Migrating from Chat Completions

Treat migration as three related changes: send requests to `/v1/responses`, read output from a typed `output` array, and choose how your application will carry state between turns.

