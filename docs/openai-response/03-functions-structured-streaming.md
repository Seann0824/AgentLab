# Migrate to the Responses API — 函数、结构化输出、流式与原生工具

> 原文拆分自 `../openai-response.md`。

### 5. Update function definitions and outputs

There are two minor, but notable, differences in how functions are defined between Chat Completions and Responses.

1. In Chat Completions, function definitions are externally tagged. In Responses, they are internally tagged.
2. In Chat Completions, functions are non-strict by default. In Responses, function schemas are normalized into strict mode by default. To keep non-strict, best-effort function calling in Responses, explicitly set `strict: false`.

The Responses API function example on the right is functionally equivalent to the Chat Completions example on the left.

#### Follow function-calling best practices

In Responses, tool calls and their outputs are two distinct types of Items that are correlated using a `call_id`. See
the [function calling docs](https://developers.openai.com/api/docs/guides/function-calling#function-tool-example) for more detail on how function calling works in Responses.

### 6. Update Structured Outputs definitions

In the Responses API, Structured Outputs definitions have moved from `response_format` to `text.format`:



<div data-content-switcher-pane data-value="chat-completions">
    <div class="hidden">Chat Completions</div>
    Structured Outputs

```bash
curl https://api.openai.com/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -d '{
  "model": "gpt-5.5",
  "messages": [
    {
      "role": "user",
      "content": "Jane, 54 years old"
    }
  ],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "person",
      "strict": true,
      "schema": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "minLength": 1
          },
          "age": {
            "type": "number",
            "minimum": 0,
            "maximum": 130
          }
        },
        "required": [
          "name",
          "age"
        ],
        "additionalProperties": false
      }
    }
  },
  "reasoning_effort": "medium"
}'
```

```python
from openai import OpenAI
client = OpenAI()

response = client.chat.completions.create(
  model="gpt-5.5",
  messages=[
    {
      "role": "user",
      "content": "Jane, 54 years old",
    }
  ],
  response_format={
    "type": "json_schema",
    "json_schema": {
      "name": "person",
      "strict": True,
      "schema": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "minLength": 1
          },
          "age": {
            "type": "number",
            "minimum": 0,
            "maximum": 130
          }
        },
        "required": [
          "name",
          "age"
        ],
        "additionalProperties": False
      }
    }
  },
  reasoning_effort="medium"
)
```

```javascript
const completion = await openai.chat.completions.create({
  model: "gpt-5.5",
  messages: [
    {
      "role": "user",
      "content": "Jane, 54 years old",
    }
  ],
  response_format: {
    type: "json_schema",
    json_schema: {
      name: "person",
      strict: true,
      schema: {
        type: "object",
        properties: {
          name: {
            type: "string",
            minLength: 1
          },
          age: {
            type: "number",
            minimum: 0,
            maximum: 130
          }
        },
        required: [
          "name",
          "age"
        ],
        additionalProperties: false
      }
    }
  },
  reasoning_effort: "medium"
});
```

  </div>
  <div data-content-switcher-pane data-value="responses" hidden>
    <div class="hidden">Responses</div>
    Structured Outputs

```bash
curl https://api.openai.com/v1/responses \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -d '{
  "model": "gpt-5.5",
  "input": "Jane, 54 years old",
  "text": {
    "format": {
      "type": "json_schema",
      "name": "person",
      "strict": true,
      "schema": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "minLength": 1
          },
          "age": {
            "type": "number",
            "minimum": 0,
            "maximum": 130
          }
        },
        "required": [
          "name",
          "age"
        ],
        "additionalProperties": false
      }
    }
  }
}'
```

```python
response = client.responses.create(
  model="gpt-5.5",
  input="Jane, 54 years old", 
  text={
    "format": {
      "type": "json_schema",
      "name": "person",
      "strict": True,
      "schema": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "minLength": 1
          },
          "age": {
            "type": "number",
            "minimum": 0,
            "maximum": 130
          }
        },
        "required": [
          "name",
          "age"
        ],
        "additionalProperties": False
      }
    }
  }
)
```

```javascript
const response = await openai.responses.create({
  model: "gpt-5.5",
  input: "Jane, 54 years old",
  text: {
    format: {
      type: "json_schema",
      name: "person",
      strict: true,
      schema: {
        type: "object",
        properties: {
          name: {
            type: "string",
            minLength: 1
          },
          age: {
            type: "number",
            minimum: 0,
            maximum: 130
          }
        },
        required: [
          "name",
          "age"
        ],
        additionalProperties: false
      }
    },
  }
});
```

  </div>



### 7. Update streaming consumers

Chat Completions streaming returns incremental chunks with a `delta` field. Responses streaming uses typed server-sent events. Update stream consumers to branch on each event's `type` and handle the events your UI or orchestration layer needs.

For text streaming, listen for events such as:

- `response.created`
- `response.output_text.delta`
- `response.completed`
- `error`

Function-calling streams can also emit events such as `response.function_call_arguments.delta` and `response.function_call_arguments.done`. See the [streaming Responses guide](https://developers.openai.com/api/docs/guides/streaming-responses?api-mode=responses) and [Responses streaming events reference](https://developers.openai.com/api/docs/api-reference/responses-streaming).

### 8. Upgrade to native tools

If your application has use cases that would benefit from OpenAI's native [tools](https://developers.openai.com/api/docs/guides/tools), you can update your tool calls to use OpenAI's tools out of the box.



<div data-content-switcher-pane data-value="chat-completions">
    <div class="hidden">Chat Completions</div>
    With Chat Completions, you cannot use OpenAI-hosted tools natively and have
    to write your own tool integration.
    Web search tool

```javascript
async function web_search(query) {
  const fetch = (await import('node-fetch')).default;
  const res = await fetch(\`https://api.example.com/search?q=\${query}\`);
  const data = await res.json();
  return data.results;
}

const completion = await client.chat.completions.create({
  model: 'gpt-5.5',
  messages: [
    { role: 'system', content: 'You are a helpful assistant.' },
    { role: 'user', content: 'Who is the current president of France?' }
  ],
  functions: [
    {
      name: 'web_search',
      description: 'Search the web for information',
      parameters: {
        type: 'object',
        properties: { query: { type: 'string' } },
        required: ['query']
      }
    }
  ]
});
```

```python
import requests

def web_search(query):
    r = requests.get(f"https://api.example.com/search?q={query}")
    return r.json().get("results", [])

completion = client.chat.completions.create(
    model="gpt-5.5",
    messages=[
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "Who is the current president of France?"}
    ],
    functions=[
        {
            "name": "web_search",
            "description": "Search the web for information",
            "parameters": {
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"]
            }
        }
    ]
)
```

```bash
curl https://api.example.com/search \\
  -G \\
  --data-urlencode "q=your+search+term" \\
  --data-urlencode "key=$SEARCH_API_KEY"\
```

  </div>
  <div data-content-switcher-pane data-value="responses" hidden>
    <div class="hidden">Responses</div>
    With Responses, you can specify the tools that you want the model to use.
    Web search tool

```javascript
const answer = await client.responses.create({
  model: 'gpt-5.5',
  input: 'Who is the current president of France?',
  tools: [{ type: 'web_search' }]
});

console.log(answer.output_text);
```

```python
answer = client.responses.create(
    model="gpt-5.5",
    input="Who is the current president of France?",
    tools=[{"type": "web_search"}]
)

print(answer.output_text)
```

```bash
curl https://api.openai.com/v1/responses \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -d '{
    "model": "gpt-5.5",
    "input": "Who is the current president of France?",
    "tools": [{"type": "web_search"}]
  }'
```


  </div>



