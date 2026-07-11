import { create } from "zustand";
import {
  chatCompletionStream,
  createChatSession,
  deleteChatSession,
  deleteNamespace,
  getChatHistory,
  indexDocumentContent,
  listChatSessions,
  listNamespaces,
  renameChatSession,
} from "../api/chatApi";
import type {
  AgentStreamEvent,
  ChatMessage,
  IndexDocumentResult,
  SessionSummary,
} from "../types/chat";

interface StreamingState {
  isStreaming: boolean;
  streamingMessageId: string | null;
}

interface ChatState {
  sessions: SessionSummary[];
  currentSessionId: string | null;
  messagesBySession: Record<string, ChatMessage[]>;
  streamingBySession: Record<string, StreamingState>;
  unreadBySession: Record<string, number>;
  namespaces: string[];

  // actions
  loadSessions: () => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  createSession: () => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  handleStreamEvent: (event: AgentStreamEvent, sessionId: string) => void;
  loadNamespaces: () => Promise<void>;
  indexDocument: (namespace: string, content: string, source: string) => Promise<IndexDocumentResult>;
  deleteNamespace: (namespace: string) => Promise<boolean>;

  // 按 session 读取的便捷 getter
  getSessionMessages: (sessionId: string) => ChatMessage[];
  getSessionStreamingState: (sessionId: string) => StreamingState;
  getSessionUnreadCount: (sessionId: string) => number;
}

function formatTitle(content: string): string {
  const trimmed = content.trim();
  if (trimmed.length === 0) return "新会话";
  if (trimmed.length <= 20) return trimmed;
  return trimmed.slice(0, 20) + "...";
}

function updateSessionTitle(
  sessions: SessionSummary[],
  sessionId: string,
  title: string,
): SessionSummary[] {
  return sessions.map((s) => (s.id === sessionId ? { ...s, title } : s));
}

function getStreamingState(
  state: Pick<ChatState, "streamingBySession">,
  sessionId: string,
): StreamingState {
  return state.streamingBySession[sessionId] ?? { isStreaming: false, streamingMessageId: null };
}

function getMessages(
  state: Pick<ChatState, "messagesBySession">,
  sessionId: string,
): ChatMessage[] {
  return state.messagesBySession[sessionId] ?? [];
}

function appendUserMessage(messages: ChatMessage[], message: ChatMessage): ChatMessage[] {
  return [...messages, message];
}

function appendOrUpdateReasonDelta(
  messages: ChatMessage[],
  messageId: string,
  delta: string,
): ChatMessage[] {
  const idx = messages.findIndex((m) => m.id === messageId);
  if (idx === -1) {
    const newMessage: ChatMessage = {
      id: messageId,
      role: "assistant",
      content: delta,
      timestamp: Math.floor(Date.now() / 1000),
      metadata: { isReasoning: true },
    };
    return [...messages, newMessage];
  }

  const next = [...messages];
  next[idx] = { ...next[idx], content: next[idx].content + delta };
  return next;
}

function appendOrUpdateAssistantDelta(
  messages: ChatMessage[],
  messageId: string,
  delta: string,
): ChatMessage[] {
  const idx = messages.findIndex((m) => m.id === messageId);

  if (idx === -1) {
    const newMessage: ChatMessage = {
      id: messageId,
      role: "assistant",
      content: delta,
      timestamp: Math.floor(Date.now() / 1000),
    };
    return [...messages, newMessage];
  }

  const existing = messages[idx];
  const next = [...messages];

  // 如果前一段是 reasoning，把 reasoning 内容移到 metadata.reason，
  // 然后开始写入正式回答内容。
  if (existing.metadata?.isReasoning) {
    next[idx] = {
      ...existing,
      content: delta,
      metadata: {
        ...existing.metadata,
        reason: existing.content,
        isReasoning: false,
      },
    };
  } else {
    next[idx] = { ...existing, content: existing.content + delta };
  }

  return next;
}

function finalizeAssistantMessage(
  messages: ChatMessage[],
  message: ChatMessage,
): ChatMessage[] {
  const idx = messages.findIndex((m) => m.id === message.id);
  if (idx >= 0) {
    const next = [...messages];
    const existing = next[idx];
    // 合并后端 metadata 与前端流式过程中积累的 metadata（主要是 reason）。
    next[idx] = {
      ...message,
      metadata: {
        ...(existing.metadata ?? {}),
        ...(message.metadata ?? {}),
        isReasoning: false,
      },
    };
    return next;
  }

  // 兜底：id 不匹配时直接追加（正常不应发生）。
  return [...messages, message];
}

export const useChatStore = create<ChatState>((set, get) => ({
  sessions: [],
  currentSessionId: null,
  messagesBySession: {},
  streamingBySession: {},
  unreadBySession: {},
  namespaces: [],

  getSessionMessages: (sessionId) => getMessages(get(), sessionId),
  getSessionStreamingState: (sessionId) => getStreamingState(get(), sessionId),
  getSessionUnreadCount: (sessionId) => get().unreadBySession[sessionId] ?? 0,

  loadSessions: async () => {
    const sessions = await listChatSessions();
    set({ sessions });
  },

  selectSession: async (id) => {
    const cached = get().messagesBySession[id];
    if (!cached || cached.length === 0) {
      const messages = await getChatHistory(id);
      set((state) => ({
        currentSessionId: id,
        messagesBySession: { ...state.messagesBySession, [id]: messages },
        unreadBySession: { ...state.unreadBySession, [id]: 0 },
      }));
    } else {
      set((state) => ({
        currentSessionId: id,
        unreadBySession: { ...state.unreadBySession, [id]: 0 },
      }));
    }
  },

  createSession: async () => {
    const id = await createChatSession();
    const newSession: SessionSummary = {
      id,
      title: "新会话",
      updated_at: Math.floor(Date.now() / 1000),
    };
    set((state) => ({
      sessions: [newSession, ...state.sessions],
      currentSessionId: id,
      messagesBySession: { ...state.messagesBySession, [id]: [] },
      streamingBySession: {
        ...state.streamingBySession,
        [id]: { isStreaming: false, streamingMessageId: null },
      },
      unreadBySession: { ...state.unreadBySession, [id]: 0 },
    }));
  },

  deleteSession: async (id) => {
    try {
      const ok = await deleteChatSession(id);
      if (!ok) {
        alert("删除会话失败，请刷新后重试");
        return;
      }
    } catch (err) {
      alert(`删除会话失败: ${err}`);
      return;
    }

    set((state) => {
      const remaining = state.sessions.filter((s) => s.id !== id);
      let nextSessionId = state.currentSessionId;

      if (state.currentSessionId === id) {
        nextSessionId = remaining[0]?.id ?? null;
      }

      const { [id]: _removedMessages, ...restMessages } = state.messagesBySession;
      const { [id]: _removedStreaming, ...restStreaming } = state.streamingBySession;
      const { [id]: _removedUnread, ...restUnread } = state.unreadBySession;

      return {
        sessions: remaining,
        currentSessionId: nextSessionId,
        messagesBySession: restMessages,
        streamingBySession: restStreaming,
        unreadBySession: restUnread,
      };
    });

    const { currentSessionId } = get();
    if (currentSessionId) {
      await get().selectSession(currentSessionId);
    }
  },

  renameSession: async (id, title) => {
    const ok = await renameChatSession(id, title);
    if (!ok) return;
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === id ? { ...s, title } : s,
      ),
    }));
  },

  sendMessage: async (content) => {
    let targetSessionId = get().currentSessionId;
    if (!targetSessionId) {
      await get().createSession();
      targetSessionId = get().currentSessionId;
      if (!targetSessionId) return;
    }

    set((state) => ({
      streamingBySession: {
        ...state.streamingBySession,
        [targetSessionId]: { isStreaming: true, streamingMessageId: null },
      },
    }));

    try {
      await chatCompletionStream(
        targetSessionId,
        content,
        (event) => {
          get().handleStreamEvent(event, targetSessionId);
        },
      );
    } finally {
      set((state) => ({
        streamingBySession: {
          ...state.streamingBySession,
          [targetSessionId]: { isStreaming: false, streamingMessageId: null },
        },
      }));
      // 刷新会话列表，使后台完成的会话排序与标题保持最新。
      await get().loadSessions();
    }
  },

  handleStreamEvent: (event, sessionId) => {
    const { currentSessionId } = get();
    const isBackground = sessionId !== currentSessionId;

    switch (event.type) {
      case "user_message": {
        set((state) => {
          const messages = getMessages(state, sessionId);
          return {
            messagesBySession: {
              ...state.messagesBySession,
              [sessionId]: appendUserMessage(messages, event.message),
            },
            sessions: updateSessionTitle(
              state.sessions,
              sessionId,
              formatTitle(event.message.content),
            ),
          };
        });
        break;
      }

      case "reason_delta": {
        set((state) => {
          const messages = getMessages(state, sessionId);
          return {
            messagesBySession: {
              ...state.messagesBySession,
              [sessionId]: appendOrUpdateReasonDelta(messages, event.message_id, event.delta),
            },
            streamingBySession: {
              ...state.streamingBySession,
              [sessionId]: {
                ...getStreamingState(state, sessionId),
                streamingMessageId: event.message_id,
              },
            },
          };
        });
        break;
      }

      case "assistant_delta": {
        set((state) => {
          const messages = getMessages(state, sessionId);
          return {
            messagesBySession: {
              ...state.messagesBySession,
              [sessionId]: appendOrUpdateAssistantDelta(messages, event.message_id, event.delta),
            },
            streamingBySession: {
              ...state.streamingBySession,
              [sessionId]: {
                ...getStreamingState(state, sessionId),
                streamingMessageId: event.message_id,
              },
            },
          };
        });
        break;
      }

      case "assistant_done": {
        set((state) => {
          const messages = getMessages(state, sessionId);
          const nextMessages = finalizeAssistantMessage(messages, event.message);
          const nextUnread = isBackground
            ? {
                ...state.unreadBySession,
                [sessionId]: (state.unreadBySession[sessionId] ?? 0) + 1,
              }
            : state.unreadBySession;

          return {
            messagesBySession: {
              ...state.messagesBySession,
              [sessionId]: nextMessages,
            },
            streamingBySession: {
              ...state.streamingBySession,
              [sessionId]: { isStreaming: false, streamingMessageId: null },
            },
            unreadBySession: nextUnread,
          };
        });
        break;
      }

      case "tool_call_start": {
        set((state) => {
          const messages = getMessages(state, sessionId);
          const exists = messages.some((m) => m.tool_call_id === event.tool_call_id);
          if (exists) return state;

          const toolMessage: ChatMessage = {
            id: event.tool_call_id,
            role: "tool",
            content: "",
            timestamp: Math.floor(Date.now() / 1000),
            tool_call_id: event.tool_call_id,
            metadata: {
              tool_name: event.tool_name,
              arguments: event.arguments,
              status: "running",
            },
          };
          return {
            messagesBySession: {
              ...state.messagesBySession,
              [sessionId]: [...messages, toolMessage],
            },
          };
        });
        break;
      }

      case "tool_call_end": {
        set((state) => {
          const messages = getMessages(state, sessionId);
          return {
            messagesBySession: {
              ...state.messagesBySession,
              [sessionId]: messages.map((m) =>
                m.tool_call_id === event.tool_call_id
                  ? {
                      ...m,
                      content: event.result,
                      metadata: {
                        ...(m.metadata ?? {}),
                        tool_name: event.tool_name,
                        status: event.is_error ? "error" : "done",
                      },
                    }
                  : m,
              ),
            },
          };
        });
        break;
      }

      case "tool_call_delta": {
        set((state) => {
          const messages = getMessages(state, sessionId);
          const idx = messages.findIndex((m) => m.tool_call_id === event.tool_call_id);
          if (idx === -1) return state;

          const next = [...messages];
          next[idx] = { ...next[idx], content: next[idx].content + event.delta };
          return {
            messagesBySession: {
              ...state.messagesBySession,
              [sessionId]: next,
            },
          };
        });
        break;
      }

      case "reason_done": {
        // reasoning 内容已通过 reason_delta 流式累积到消息 content 中，
        // 这里不需要额外更新；metadata.reason 会在 assistant_delta 切换时写入。
        break;
      }
    }
  },

  loadNamespaces: async () => {
    const namespaces = await listNamespaces();
    set({ namespaces });
  },

  indexDocument: async (namespace, content, source) => {
    const result = await indexDocumentContent(namespace, content, source);
    if (!result.already_exists) {
      await get().loadNamespaces();
    }
    return result;
  },

  deleteNamespace: async (namespace) => {
    const ok = await deleteNamespace(namespace);
    if (ok) {
      await get().loadNamespaces();
    }
    return ok;
  },
}));
