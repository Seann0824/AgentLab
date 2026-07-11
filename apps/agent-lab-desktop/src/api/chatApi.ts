import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AgentStreamEvent,
  ChatMessage,
  IndexDocumentResult,
  SessionSummary,
} from "../types/chat";

export async function listNamespaces(): Promise<string[]> {
  return invoke<string[]>("list_namespaces");
}

export async function indexDocumentContent(
  namespace: string,
  content: string,
  source: string,
): Promise<IndexDocumentResult> {
  return invoke<IndexDocumentResult>("index_document_content", {
    namespace,
    content,
    source,
  });
}

export async function deleteNamespace(namespace: string): Promise<boolean> {
  return invoke<boolean>("delete_namespace", { namespace });
}

export async function listChatSessions(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("list_chat_sessions");
}

export async function getChatHistory(sessionId: string): Promise<ChatMessage[]> {
  return invoke<ChatMessage[]>("get_chat_history", { sessionId });
}

export async function createChatSession(): Promise<string> {
  return invoke<string>("create_chat_session");
}

export async function deleteChatSession(sessionId: string): Promise<boolean> {
  return invoke<boolean>("delete_chat_session", { sessionId });
}

export async function renameChatSession(
  sessionId: string,
  title: string,
): Promise<boolean> {
  return invoke<boolean>("rename_chat_session", { sessionId, title });
}

export async function chatCompletionStream(
  sessionId: string | null,
  message: string,
  onEvent: (event: AgentStreamEvent) => void,
): Promise<string> {
  const channel = new Channel<AgentStreamEvent>((event) => {
    onEvent(event);
  });

  return invoke<string>("chat_completion_stream", {
    sessionId,
    message,
    channel,
  });
}
