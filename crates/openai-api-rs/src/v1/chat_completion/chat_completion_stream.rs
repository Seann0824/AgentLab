use crate::v1::chat_completion::{
    FinishReason, Reasoning, ReasoningEffort, Tool, ToolCall, ToolCallFunction, ToolChoiceType,
};
use crate::{
    impl_builder_methods,
    v1::chat_completion::{serialize_tool_choice, ChatCompletionMessage},
};

use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionStreamRequest {
    pub model: String,
    pub messages: Vec<ChatCompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<HashMap<String, i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "serialize_tool_choice")]
    pub tool_choice: Option<ToolChoiceType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Optional list of transforms to apply to the chat completion request.
    ///
    /// Transforms allow modifying the request before it's sent to the API,
    /// enabling features like prompt rewriting, content filtering, or other
    /// preprocessing steps. When None, no transforms are applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transforms: Option<Vec<String>>,
}

impl ChatCompletionStreamRequest {
    pub fn new(model: String, messages: Vec<ChatCompletionMessage>) -> Self {
        Self {
            model,
            messages,
            temperature: None,
            top_p: None,
            n: None,
            response_format: None,
            stop: None,
            max_tokens: None,
            presence_penalty: None,
            frequency_penalty: None,
            logit_bias: None,
            user: None,
            seed: None,
            tools: None,
            parallel_tool_calls: None,
            tool_choice: None,
            reasoning: None,
            reasoning_effort: None,
            transforms: None,
        }
    }
}

impl_builder_methods!(
    ChatCompletionStreamRequest,
    temperature: f64,
    top_p: f64,
    n: i64,
    response_format: Value,
    stop: Vec<String>,
    max_tokens: i64,
    presence_penalty: f64,
    frequency_penalty: f64,
    logit_bias: HashMap<String, i32>,
    user: String,
    seed: i64,
    tools: Vec<Tool>,
    parallel_tool_calls: bool,
    tool_choice: ToolChoiceType,
    reasoning: Reasoning,
    reasoning_effort: ReasoningEffort,
    transforms: Vec<String>
);

#[derive(Debug, Clone)]
pub enum ChatCompletionStreamResponse {
    Content(String),
    Reasoning(String),
    ToolCall(Vec<ToolCall>),
    Done(Option<FinishReason>),
}

pub struct ChatCompletionStream<S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin> {
    pub response: S,
    pub buffer: String,
    pub first_chunk: bool,
    tool_calls: BTreeMap<usize, PartialToolCall>,
    emit_done_after_tool_calls: bool,
    finish_reason: Option<FinishReason>,
}

#[derive(Debug, Default, Clone)]
struct PartialToolCall {
    id: Option<String>,
    r#type: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl PartialToolCall {
    fn apply_delta(&mut self, delta: &Value) {
        if let Some(id) = delta.get("id").and_then(|id| id.as_str()) {
            if !id.is_empty() {
                self.id = Some(id.to_string());
            }
        }

        if let Some(tool_type) = delta.get("type").and_then(|tool_type| tool_type.as_str()) {
            if !tool_type.is_empty() {
                self.r#type = Some(tool_type.to_string());
            }
        }

        if let Some(function) = delta.get("function") {
            if let Some(name) = function.get("name").and_then(|name| name.as_str()) {
                if !name.is_empty() {
                    self.name = Some(name.to_string());
                }
            }

            if let Some(arguments) = function
                .get("arguments")
                .and_then(|arguments| arguments.as_str())
            {
                self.arguments.push_str(arguments);
            }
        }
    }

    fn into_tool_call(self) -> Option<ToolCall> {
        Some(ToolCall {
            id: self.id?,
            r#type: self.r#type.unwrap_or_else(|| "function".to_string()),
            function: ToolCallFunction {
                name: self.name,
                arguments: Some(self.arguments),
            },
        })
    }
}

impl<S> ChatCompletionStream<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    pub fn new(response: S) -> Self {
        Self {
            response,
            buffer: String::new(),
            first_chunk: true,
            tool_calls: BTreeMap::new(),
            emit_done_after_tool_calls: false,
            finish_reason: None,
        }
    }

    fn find_event_delimiter(buffer: &str) -> Option<(usize, usize)> {
        let carriage_idx = buffer.find("\r\n\r\n");
        let newline_idx = buffer.find("\n\n");

        match (carriage_idx, newline_idx) {
            (Some(r_idx), Some(n_idx)) => {
                if r_idx <= n_idx {
                    Some((r_idx, 4))
                } else {
                    Some((n_idx, 2))
                }
            }
            (Some(r_idx), None) => Some((r_idx, 4)),
            (None, Some(n_idx)) => Some((n_idx, 2)),
            (None, None) => None,
        }
    }

    fn apply_tool_call_delta(&mut self, tool_call_delta: &Value) {
        if let Some(index) = tool_call_delta
            .get("index")
            .and_then(|index| index.as_u64())
        {
            self.tool_calls
                .entry(index as usize)
                .or_default()
                .apply_delta(tool_call_delta);
            return;
        }

        if let Ok(tool_call) = serde_json::from_value::<ToolCall>(tool_call_delta.clone()) {
            let index = self.tool_calls.len();
            self.tool_calls.insert(
                index,
                PartialToolCall {
                    id: Some(tool_call.id),
                    r#type: Some(tool_call.r#type),
                    name: tool_call.function.name,
                    arguments: tool_call.function.arguments.unwrap_or_default(),
                },
            );
        }
    }

    fn take_tool_calls(&mut self) -> Vec<ToolCall> {
        std::mem::take(&mut self.tool_calls)
            .into_values()
            .filter_map(PartialToolCall::into_tool_call)
            .collect()
    }

    fn finish_reason_from_choice(choice: &Value) -> Option<FinishReason> {
        let finish_reason = choice.get("finish_reason")?;
        if finish_reason.is_null() {
            return None;
        }

        match serde_json::from_value(finish_reason.clone()) {
            Ok(finish_reason) => Some(finish_reason),
            Err(error) => {
                eprintln!("Failed to parse finish_reason: {}", error);
                None
            }
        }
    }

    fn next_response_from_buffer(&mut self) -> Option<ChatCompletionStreamResponse> {
        if self.emit_done_after_tool_calls {
            self.emit_done_after_tool_calls = false;
            return Some(ChatCompletionStreamResponse::Done(
                self.finish_reason.take(),
            ));
        }

        while let Some((idx, delimiter_len)) = Self::find_event_delimiter(&self.buffer) {
            let event = self.buffer[..idx].to_owned();
            self.buffer = self.buffer[idx + delimiter_len..].to_owned();

            let mut data_payload = String::new();
            for line in event.lines() {
                let trimmed_line = line.trim_end_matches('\r');
                if let Some(content) = trimmed_line
                    .strip_prefix("data: ")
                    .or_else(|| trimmed_line.strip_prefix("data:"))
                {
                    if !content.is_empty() {
                        if !data_payload.is_empty() {
                            data_payload.push('\n');
                        }
                        data_payload.push_str(content);
                    }
                }
            }

            if data_payload.is_empty() {
                continue;
            }

            if data_payload == "[DONE]" {
                if !self.tool_calls.is_empty() {
                    if self.finish_reason.is_none() {
                        self.finish_reason = Some(FinishReason::tool_calls);
                    }
                    let tool_calls = self.take_tool_calls();
                    if !tool_calls.is_empty() {
                        self.emit_done_after_tool_calls = true;
                        return Some(ChatCompletionStreamResponse::ToolCall(tool_calls));
                    }
                }

                return Some(ChatCompletionStreamResponse::Done(
                    self.finish_reason.take(),
                ));
            }

            match serde_json::from_str::<Value>(&data_payload) {
                Ok(json) => {
                    if let Some(choice) = json.get("choices").and_then(|choices| choices.get(0)) {
                        let finish_reason = Self::finish_reason_from_choice(choice);
                        let is_tool_call_finish =
                            matches!(finish_reason.as_ref(), Some(FinishReason::tool_calls));
                        if let Some(finish_reason) = finish_reason {
                            self.finish_reason = Some(finish_reason);
                        }

                        if let Some(delta) = choice.get("delta") {
                            if let Some(tool_calls) = delta
                                .get("tool_calls")
                                .and_then(|tool_calls| tool_calls.as_array())
                            {
                                for tool_call_delta in tool_calls {
                                    self.apply_tool_call_delta(tool_call_delta);
                                }
                            }

                            if is_tool_call_finish {
                                let tool_calls = self.take_tool_calls();
                                if !tool_calls.is_empty() {
                                    return Some(ChatCompletionStreamResponse::ToolCall(
                                        tool_calls,
                                    ));
                                }
                            }

                            if !self.tool_calls.is_empty() {
                                continue;
                            }

                            if let Some(reasoning) = delta
                                .get("reasoning")
                                .or_else(|| delta.get("reasoning_content"))
                                .and_then(|r| r.as_str())
                            {
                                let output = reasoning.replace("\\n", "\n");
                                return Some(ChatCompletionStreamResponse::Reasoning(output));
                            }

                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                let output = content.replace("\\n", "\n");
                                return Some(ChatCompletionStreamResponse::Content(output));
                            }
                        }

                        if is_tool_call_finish {
                            let tool_calls = self.take_tool_calls();
                            if !tool_calls.is_empty() {
                                return Some(ChatCompletionStreamResponse::ToolCall(tool_calls));
                            }
                        }
                    }
                }
                Err(error) => {
                    eprintln!("Failed to parse SSE chunk as JSON: {}", error);
                }
            }
        }

        None
    }
}

impl<S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin> Stream
    for ChatCompletionStream<S>
{
    type Item = ChatCompletionStreamResponse;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(response) = self.next_response_from_buffer() {
                return Poll::Ready(Some(response));
            }

            match Pin::new(&mut self.as_mut().response).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    let chunk_str = String::from_utf8_lossy(&chunk).to_string();

                    if self.first_chunk {
                        self.first_chunk = false;
                    }
                    self.buffer.push_str(&chunk_str);
                }
                Poll::Ready(Some(Err(error))) => {
                    eprintln!("Error in stream: {:?}", error);
                    return Poll::Ready(None);
                }
                Poll::Ready(None) => {
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::v1::chat_completion::{ReasoningEffort, ReasoningSummary};

    use super::*;
    use serde_json::json;

    #[test]
    fn test_reasoning_effort_serialization() {
        let reasoning = Reasoning {
            effort: Some(ReasoningEffort::High),
            summary: Some(ReasoningSummary::Detailed),
        };

        let serialized = serde_json::to_value(&reasoning).unwrap();
        let expected = json!({
            "effort": "high",
            "summary": "detailed"
        });

        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_reasoning_summary_only_serialization() {
        let reasoning = Reasoning {
            effort: None,
            summary: Some(ReasoningSummary::Auto),
        };

        let serialized = serde_json::to_value(&reasoning).unwrap();
        let expected = json!({
            "summary": "auto"
        });

        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_reasoning_deserialization() {
        let json_str = r#"{"effort": "medium", "summary": "concise"}"#;
        let reasoning: Reasoning = serde_json::from_str(json_str).unwrap();
        assert_eq!(reasoning.effort, Some(ReasoningEffort::Medium));
        assert_eq!(reasoning.summary, Some(ReasoningSummary::Concise));
    }

    #[test]
    fn test_chat_completion_request_with_reasoning() {
        let mut req = ChatCompletionStreamRequest::new("gpt-4".to_string(), vec![]);

        req.reasoning = Some(Reasoning {
            effort: Some(ReasoningEffort::Low),
            summary: Some(ReasoningSummary::Auto),
        });

        let serialized = serde_json::to_value(&req).unwrap();
        assert_eq!(serialized["reasoning"]["effort"], "low");
        assert_eq!(serialized["reasoning"]["summary"], "auto");
    }

    #[test]
    fn test_chat_completion_stream_request_with_reasoning_effort() {
        let mut req = ChatCompletionStreamRequest::new("gpt-5.1".to_string(), vec![]);
        req.reasoning_effort = Some(ReasoningEffort::None);

        let serialized = serde_json::to_value(&req).unwrap();
        assert_eq!(serialized["reasoning_effort"], "none");
    }

    #[test]
    fn test_transforms_none_serialization() {
        let req = ChatCompletionStreamRequest::new("gpt-4".to_string(), vec![]);
        let serialised = serde_json::to_value(&req).unwrap();
        // Verify that the transforms field is completely omitted from JSON output
        assert!(!serialised.as_object().unwrap().contains_key("transforms"));
    }

    #[test]
    fn test_transforms_some_serialization() {
        let mut req = ChatCompletionStreamRequest::new("gpt-4".to_string(), vec![]);
        req.transforms = Some(vec!["transform1".to_string(), "transform2".to_string()]);
        let serialised = serde_json::to_value(&req).unwrap();
        // Verify that the transforms field is included as a proper JSON array
        assert_eq!(
            serialised["transforms"],
            serde_json::json!(["transform1", "transform2"])
        );
    }

    #[test]
    fn test_transforms_some_deserialization() {
        let json_str =
            r#"{"model": "gpt-4", "messages": [], "transforms": ["transform1", "transform2"]}"#;
        let req: ChatCompletionStreamRequest = serde_json::from_str(json_str).unwrap();
        // Verify that the transforms field is properly populated with Some(vec)
        assert_eq!(
            req.transforms,
            Some(vec!["transform1".to_string(), "transform2".to_string()])
        );
    }

    #[test]
    fn test_transforms_none_deserialization() {
        let json_str = r#"{"model": "gpt-4", "messages": []}"#;
        let req: ChatCompletionStreamRequest = serde_json::from_str(json_str).unwrap();
        // Verify that the transforms field is properly set to None when absent
        assert_eq!(req.transforms, None);
    }

    #[test]
    fn test_transforms_builder_method() {
        let transforms = vec!["transform1".to_string(), "transform2".to_string()];
        let req = ChatCompletionStreamRequest::new("gpt-4".to_string(), vec![])
            .transforms(transforms.clone());
        // Verify that the transforms field is properly set through the builder method
        assert_eq!(req.transforms, Some(transforms));
    }

    #[test]
    fn test_reasoning_effort_builder_method() {
        let req = ChatCompletionStreamRequest::new("gpt-5.1".to_string(), vec![])
            .reasoning_effort(ReasoningEffort::Xhigh);
        assert_eq!(req.reasoning_effort, Some(ReasoningEffort::Xhigh));
    }

    #[test]
    fn test_stream_reasoning_delta() {
        let mut stream = ChatCompletionStream::new(futures_util::stream::empty());
        stream.buffer =
            "data: {\"choices\":[{\"delta\":{\"reasoning\":\"step 1\"}}]}\n\n".to_string();
        stream.first_chunk = false;

        let response = stream.next_response_from_buffer();
        match response {
            Some(ChatCompletionStreamResponse::Reasoning(reasoning)) => {
                assert_eq!(reasoning, "step 1");
            }
            _ => panic!("Expected reasoning delta"),
        }
    }

    #[test]
    fn test_finish_reason_function_call_deserializes() {
        let finish_reason: FinishReason = serde_json::from_str(r#""function_call""#).unwrap();
        assert_eq!(finish_reason, FinishReason::function_call);
    }

    #[test]
    fn test_stream_done_includes_finish_reason() {
        let mut stream = ChatCompletionStream::new(futures_util::stream::empty());
        stream.buffer = [
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {
                            "content": "hello"
                        },
                        "finish_reason": null
                    }]
                })
            ),
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {},
                        "finish_reason": "stop"
                    }]
                })
            ),
            "data: [DONE]\n\n".to_string(),
        ]
        .concat();
        stream.first_chunk = false;

        assert!(matches!(
            stream.next_response_from_buffer(),
            Some(ChatCompletionStreamResponse::Content(content)) if content == "hello"
        ));

        assert!(matches!(
            stream.next_response_from_buffer(),
            Some(ChatCompletionStreamResponse::Done(Some(FinishReason::stop)))
        ));
    }

    #[test]
    fn test_stream_tool_call_arguments_are_accumulated() {
        let mut stream = ChatCompletionStream::new(futures_util::stream::empty());
        stream.buffer = [
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "web_search",
                                    "arguments": ""
                                }
                            }]
                        },
                        "finish_reason": null
                    }]
                })
            ),
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "function": {
                                    "arguments": "{\"query\""
                                }
                            }]
                        },
                        "finish_reason": null
                    }]
                })
            ),
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "function": {
                                    "arguments": ":\"London weather\"}"
                                }
                            }]
                        },
                        "finish_reason": null
                    }]
                })
            ),
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {},
                        "finish_reason": "tool_calls"
                    }]
                })
            ),
            "data: [DONE]\n\n".to_string(),
        ]
        .concat();
        stream.first_chunk = false;

        let response = stream.next_response_from_buffer();
        match response {
            Some(ChatCompletionStreamResponse::ToolCall(tool_calls)) => {
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call_1");
                assert_eq!(tool_calls[0].r#type, "function");
                assert_eq!(tool_calls[0].function.name.as_deref(), Some("web_search"));
                assert_eq!(
                    tool_calls[0].function.arguments.as_deref(),
                    Some("{\"query\":\"London weather\"}")
                );
            }
            _ => panic!("Expected complete tool call"),
        }

        assert!(matches!(
            stream.next_response_from_buffer(),
            Some(ChatCompletionStreamResponse::Done(Some(
                FinishReason::tool_calls
            )))
        ));
    }
}
