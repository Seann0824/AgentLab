# Migrate to the Responses API — 端点、消息和多轮状态迁移

> 原文拆分自 `../openai-response.md`。

### 1. Update generation endpoints

Start by updating your generation endpoints from `post /v1/chat/completions` to `post /v1/responses`.

If you are not using functions or multimodal inputs, simple message inputs are compatible from one API to the other:

Reuse simple message input

```bash
INPUT='[
  { "role": "system", "content": "You are a helpful assistant." },
  { "role": "user", "content": "Hello!" }
]'

curl -s https://api.openai.com/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -d "{
    \\"model\\": \\"gpt-5.5\\",
    \\"messages\\": $INPUT
  }"

curl -s https://api.openai.com/v1/responses \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -d "{
    \\"model\\": \\"gpt-5.5\\",
    \\"input\\": $INPUT
  }"
```

```javascript
const context = [
  { role: 'system', content: 'You are a helpful assistant.' },
  { role: 'user', content: 'Hello!' }
];

const completion = await client.chat.completions.create({
  model: 'gpt-5.5',
  messages: context
});

const response = await client.responses.create({
  model: "gpt-5.5",
  input: context
});
```

```python
context = [
  { "role": "system", "content": "You are a helpful assistant." },
  { "role": "user", "content": "Hello!" }
]

completion = client.chat.completions.create(
  model="gpt-5.5",
  messages=context
)

response = client.responses.create(
  model="gpt-5.5",
  input=context
)
```




<div data-content-switcher-pane data-value="chat-completions">
    <div class="hidden">Chat Completions</div>
    With Chat Completions, you create a `messages` array and read the model text
    from `completion.choices[0].message.content`.
    Generate text from a model

```javascript
import OpenAI from 'openai';
const client = new OpenAI({ apiKey: process.env.OPENAI_API_KEY });

const completion = await client.chat.completions.create({
  model: 'gpt-5.5',
  messages: [
    { 'role': 'system', 'content': 'You are a helpful assistant.' },
    { 'role': 'user', 'content': 'Hello!' }
  ]
});
console.log(completion.choices[0].message.content);
```

```python
from openai import OpenAI
client = OpenAI()

completion = client.chat.completions.create(
    model="gpt-5.5",
    messages=[
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "Hello!"}
    ]
)
print(completion.choices[0].message.content)
```

```bash
curl https://api.openai.com/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -d '{
      "model": "gpt-5.5",
      "messages": [
          {"role": "system", "content": "You are a helpful assistant."},
          {"role": "user", "content": "Hello!"}
      ]
  }'
```


  </div>
  <div data-content-switcher-pane data-value="responses" hidden>
    <div class="hidden">Responses</div>
    With Responses, you can separate `instructions` and `input` at the top level
    and read generated text from `response.output_text`.
    Generate text from a model

```javascript
import OpenAI from 'openai';
const client = new OpenAI({ apiKey: process.env.OPENAI_API_KEY });

const response = await client.responses.create({
  model: 'gpt-5.5',
  instructions: 'You are a helpful assistant.',
  input: 'Hello!'
});

console.log(response.output_text);
```

```python
from openai import OpenAI
client = OpenAI()

response = client.responses.create(
    model="gpt-5.5",
    instructions="You are a helpful assistant.",
    input="Hello!"
)
print(response.output_text)
```

```bash
curl https://api.openai.com/v1/responses \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -d '{
      "model": "gpt-5.5",
      "instructions": "You are a helpful assistant.",
      "input": "Hello!"
  }'
```


  </div>



### 2. Map Messages to Items

Chat Completions uses `messages` as both input and output. Responses uses `input` and `output` arrays of typed Items. A `message` is one Item type, alongside Items such as `reasoning`, `function_call`, and `function_call_output`.

| Chat Completions concept      | Responses mapping                                                                                      |
| ----------------------------- | ------------------------------------------------------------------------------------------------------ |
| `messages[]`                  | `input`, as a string or an array of input Items                                                        |
| System or developer guidance  | Top-level `instructions`, or compatible message Items when you need to preserve an existing transcript |
| User message                  | An input message Item with `role: "user"`                                                              |
| Assistant message             | An output message Item in `response.output`; pass it back in `input` if you manually manage state      |
| Tool or function call         | A `function_call` output Item                                                                          |
| Tool or function result       | A `function_call_output` input Item linked to the call with `call_id`                                  |
| Multiple generations with `n` | Not available in Responses; make separate requests if you need multiple candidate outputs              |

When you only need the final text, use the SDK `output_text` helper. When your flow uses reasoning, tools, or multimodal output, iterate over `response.output` and handle each Item by its `type`.

### 3. Update multi-turn conversations

If you have multi-turn conversations in your application, update your context logic. Responses gives you three common state-management options:

- Use `previous_response_id` when you want OpenAI to manage prior response context. Resend stable `instructions` on each request, because `previous_response_id` does not carry over the previous response's top-level `instructions`.
- Pass prior `output` Items back into the next request when you need to manage or trim context yourself.
- Use the [Conversations API](https://developers.openai.com/api/docs/guides/conversation-state?api-mode=responses#using-the-conversations-api) when you need a persistent conversation object.



<div data-content-switcher-pane data-value="chat-completions">
    <div class="hidden">Chat Completions</div>
    In Chat Completions, you store the transcript and send the accumulated
    `messages` array on each request.
    Multi-turn conversation

```javascript
let messages = [
    { 'role': 'system', 'content': 'You are a helpful assistant.' },
    { 'role': 'user', 'content': 'What is the capital of France?' }
  ];
const res1 = await client.chat.completions.create({
  model: 'gpt-5.5',
  messages
});

messages = messages.concat([res1.choices[0].message]);
messages.push({ 'role': 'user', 'content': 'And its population?' });

const res2 = await client.chat.completions.create({
  model: 'gpt-5.5',
  messages
});
```

```python
messages = [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "What is the capital of France?"}
]
res1 = client.chat.completions.create(model="gpt-5.5", messages=messages)

messages += [res1.choices[0].message]
messages += [{"role": "user", "content": "And its population?"}]

res2 = client.chat.completions.create(model="gpt-5.5", messages=messages)
```


  </div>
  <div data-content-switcher-pane data-value="responses" hidden>
    <div class="hidden">Responses</div>
    With Responses, you can manually pass outputs from one response into the
    input of another.
    Multi-turn conversation

```python
context = [
    { "role": "user", "content": "What is the capital of France?" }
]
res1 = client.responses.create(
    model="gpt-5.5",
    input=context,
)

# Append the first response's output to context
context += res1.output

# Add the next user message
context += [
    { "role": "user", "content": "And its population?" }
]

res2 = client.responses.create(
    model="gpt-5.5",
    input=context,
)
```

```javascript
let context = [
  { role: "user", content: "What is the capital of France?" }
];

const res1 = await client.responses.create({
  model: "gpt-5.5",
  input: context,
});

// Append the first response’s output to context
context = context.concat(res1.output);

// Add the next user message
context.push({ role: "user", content: "And its population?" });

const res2 = await client.responses.create({
  model: "gpt-5.5",
  input: context,
});
```

    You can also use `previous_response_id` to reference the previous response
    and create response chains or forks.
    Multi-turn conversation

```javascript
const res1 = await client.responses.create({
  model: 'gpt-5.5',
  input: 'What is the capital of France?',
  store: true
});

const res2 = await client.responses.create({
  model: 'gpt-5.5',
  input: 'And its population?',
  previous_response_id: res1.id,
  store: true
});
```

```python
res1 = client.responses.create(
    model="gpt-5.5",
    input="What is the capital of France?",
    store=True
)

res2 = client.responses.create(
    model="gpt-5.5",
    input="And its population?",
    previous_response_id=res1.id,
    store=True
)
```


  </div>



Even when using `previous_response_id`, all previous input tokens for responses in the chain are billed as input tokens in the API.

### 4. Decide when to use statefulness

Responses are stored by default. Chat Completions are stored by default for new accounts. To disable storage in either API, set `store: false`.

Some organizations, such as those with Zero Data Retention (ZDR) requirements, cannot use the Responses API in a stateful way due to compliance or data retention policies. To support these cases, OpenAI offers encrypted reasoning items, allowing you to keep your workflow stateless while still benefiting from reasoning items.

To disable statefulness but still take advantage of reasoning:

- Set `store: false` in the [store field](https://developers.openai.com/api/docs/api-reference/responses/create#responses_create-store).
- Add `["reasoning.encrypted_content"]` to the [include field](https://developers.openai.com/api/docs/api-reference/responses/create#responses_create-include).

The API will then return an encrypted version of the reasoning tokens, which you can pass back in future requests just like regular reasoning items.
For ZDR organizations, OpenAI enforces `store: false` automatically. When a request includes `encrypted_content`, it is decrypted in memory, used for generating the next response, and then securely discarded. Any new reasoning tokens are immediately encrypted and returned to you, ensuring no intermediate state is persisted.

