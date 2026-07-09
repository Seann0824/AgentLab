export interface ToolCallInfo {
  id: string;
  name: string;
  arguments: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "tool" | "system";
  content: string;
  timestamp: number;
  tool_call_id?: string;
  tool_calls?: ToolCallInfo[];
  metadata?: Record<string, unknown>;
}

export interface SessionSummary {
  id: string;
  title: string;
  updated_at: number;
}

export type AgentStreamEvent =
  | { type: "user_message"; message: ChatMessage }
  | { type: "assistant_delta"; message_id: string; delta: string }
  | { type: "assistant_done"; message: ChatMessage }
  | {
      type: "tool_call_start";
      tool_call_id: string;
      tool_name: string;
      arguments: string;
    }
  | {
      type: "tool_call_end";
      tool_call_id: string;
      tool_name: string;
      result: string;
      is_error: boolean;
    }
  | { type: "tool_call_delta"; tool_call_id: string; delta: string }
  | { type: "reason_delta"; delta: string }
  | { type: "reason_done"; reason: string };
