use std::collections::HashMap;

use tokio::sync::mpsc;
use futures_util::StreamExt;
use serde_json::json;
use tokio_stream::wrappers::ReceiverStream;

use crate::model::{ModelEvent, types::ModelStream};

use super::{ChatMessage, ModelAdapter, ToolCall};

pub struct OpenAiCompatibleAdapter {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleAdapter {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

impl ModelAdapter for OpenAiCompatibleAdapter {
    fn stream_chat(&self, messages: &Vec<ChatMessage>, tools_schema: serde_json::Value) -> ModelStream {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let (tx, rx) = mpsc::channel(100);
        let messages = openai_messages(messages);

        let params = json!({
            "model": &self.model,
            "stream": true,
            "messages": messages,
            "tools": tools_schema,
            "tool_choice": "auto",
        });

        // 感觉应该先将 messages 做转换，不过这里看着如果所有模型格式都统一的话无所
        let query_model = self.client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&params)
            .send();

        tokio::spawn(async move {
            match query_model.await {
                Result::Ok(stream) => {
                    let mut stream = stream.bytes_stream();
                    let mut buffer = String::new();
                    let mut tool_map: HashMap<usize, ModelEvent> = HashMap::new();
                    let mut assistant_message = String::new();

                    while let Some(chunck) = stream.next().await {
                        if let Result::Ok(bytes) = chunck {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }

                        while let Some(pos) = buffer.find('\n') {
                            let mut line = buffer[..pos].to_string();
                            buffer.drain(..pos + 1);
                            line = line.trim_end_matches('\r').to_string();
                            if line.is_empty() {
                                continue
                            }

                            let Some(mut data) = line.strip_prefix("data: ") else {
                                continue
                            };
                            
                            data = data.trim();
                            if data == "[DONE]" {
                                tx.send(ModelEvent::Done(assistant_message)).await;
                                return;
                            }

                            match serde_json::from_str::<serde_json::Value>(data) {
                                Result::Ok(value) => {
                                    if let Some(content) = value["choices"][0]["delta"]["content"].as_str() && !content.is_empty() {
                                        assistant_message.push_str(content);
                                        tx.send(ModelEvent::Text(content.to_string())).await;
                                    }

                                    if let Some(reasoning_content) = value["choices"][0]["delta"]["reasoning_content"].as_str() && !reasoning_content.is_empty() {
                                        tx.send(ModelEvent::Thinking(reasoning_content.to_string())).await;
                                    }
                                    // 工具调用结束直接向外发送事件
                                    // 结束条件要么是 finish 要么是 id 出现。index 作为来区分工具
                                    if let Some(finish_reason) = value["choices"][0]["finish_reason"].as_str() && finish_reason == "tool_calls" {
                                        for (_, model_event) in tool_map.into_iter() {
                                            match model_event {
                                                ModelEvent::ToolCallBlock { .. } => {
                                                    tx.send(model_event).await;
                                                }
                                                _ => ()
                                            }
                                        }
                                        return;
                                    }
                                    if let Some(tool_calls) = value["choices"][0]["delta"]["tool_calls"].as_array() {
                                        for data in tool_calls.iter() {
                                            // 判断当前索引是否创建
                                            let Some(index) = data["index"].as_u64() else {
                                                continue
                                            };
                                            match tool_map.get_mut(&(index as usize)) {
                                                Some(tool_call_block) => {
                                                    match tool_call_block {
                                                        ModelEvent::ToolCallBlock {  arguments, .. } => {
                                                            if let Some(delta) = data["function"]["arguments"].as_str() {
                                                                arguments.push_str(delta);
                                                            }
                                                        }
                                                        _ => ()
                                                    }
                                                }
                                                None => {
                                                    // 创建一个
                                                    if let (Some(id), Some(name), Some(delta)) = (
                                                        data["id"].as_str(),
                                                        data["function"]["name"].as_str(),
                                                        data["function"]["arguments"].as_str()
                                                    ) {
                                                        tool_map.insert(index as usize, ModelEvent::ToolCallBlock { id: id.to_string(), name: name.to_string(), arguments: delta.to_string() });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                                Err(_) => {
                                    tx.send(ModelEvent::Error("json parse error".to_string())).await;
                                }
                            }
                        }
                    }
                },
                Err(_) => {
                    tx.send(ModelEvent::Error("request error".to_string())).await;
                }
            }
        });
        
        Box::pin(ReceiverStream::new(rx))
    }
}

fn openai_messages(messages: &Vec<ChatMessage>) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|message| {
            match message {
                ChatMessage::System { content } => {
                    json!({
                        "role": "system",
                        "content": content,
                    })
                },
                ChatMessage::User { content } => {
                    json!({
                        "role": "user",
                        "content": content,
                    })
                }
                ChatMessage::Assistant {
                    content,
                    tool_calls,
                } => {
                    if tool_calls.len() == 0 {
                        json!({
                            "role": "assistant",
                            "content": content,
                        })
                    } else {
                        let content = if content.is_empty() {
                            serde_json::Value::Null
                        } else {
                            json!(content)
                        };

                        json!({
                            "role": "assistant",
                            "content": content,
                            "tool_calls": openai_tool_calls(tool_calls),
                        })
                    }
                }
                ChatMessage::Tool {
                    tool_call_id,
                    content,
                } => {
                    json!({
                        "role": "tool",
                        "tool_call_id": tool_call_id,
                        "content": content,
                    })
                }
            }
        })
        .collect()
}

fn openai_tool_calls(tool_calls: &Vec<ToolCall>) -> Vec<serde_json::Value> {
    tool_calls
        .iter()
        .map(|tool_call| {
            json!({
                "id": tool_call.id,
                "type": "function",
                "function": {
                    "name": tool_call.name,
                    "arguments": tool_call.arguments,
                }
            })
        })
        .collect()
}
