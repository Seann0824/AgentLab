import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AgentStreamEvent,
  ChatMessage,
  IndexDocumentResult,
  ModelSelection,
  ProviderConfig,
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
  modelSelection: ModelSelection | null,
  onEvent: (event: AgentStreamEvent) => void,
): Promise<string> {
  const channel = new Channel<AgentStreamEvent>((event) => {
    onEvent(event);
  });

  return invoke<string>("chat_completion_stream", {
    sessionId,
    message,
    modelSelection,
    channel,
  });
}

// 模型配置相关 API
export async function listProviders(): Promise<ProviderConfig[]> {
  return invoke<ProviderConfig[]>("list_providers");
}

export async function saveProvider(
  config: ProviderConfig,
): Promise<ProviderConfig[]> {
  return invoke<ProviderConfig[]>("save_provider", { config });
}

export async function deleteProvider(id: string): Promise<ProviderConfig[]> {
  return invoke<ProviderConfig[]>("delete_provider", { id });
}

export async function getDefaultModel(): Promise<ModelSelection> {
  return invoke<ModelSelection>("get_default_model");
}

export async function setDefaultModel(
  selection: ModelSelection,
): Promise<void> {
  return invoke("set_default_model", { selection });
}
