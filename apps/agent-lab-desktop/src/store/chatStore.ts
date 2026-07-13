import { create } from "zustand";
import {
  chatCompletionStream,
  deleteChatSession,
  deleteNamespace,
  getChatHistory,
  getDefaultModel,
  getMemoryEnabled,
  indexDocumentContent,
  listChatSessions,
  listNamespaces,
  listProviders,
  renameChatSession,
  saveProvider,
  deleteProvider,
  setDefaultModel,
  setMemoryEnabled,
} from "../api/chatApi";
import type {
  AgentStreamEvent,
  ChatMessage,
  IndexDocumentResult,
  ModelSelection,
  ProviderConfig,
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
  providers: ProviderConfig[];
  defaultModel: ModelSelection | null;
  memoryEnabled: boolean;
  selectedModelBySession: Record<string, ModelSelection>;
  // 虚拟 session 的模型选择：在没有真实 session 时，用户仍可提前选择模型，
  // 待发送第一条消息创建真实 session 后自动转移过去。
  pendingSessionModel: ModelSelection | null;
  // 侧边栏折叠状态
  isSidebarCollapsed: boolean;

  // actions
  loadSessions: () => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  startNewChat: () => void;
  deleteSession: (id: string) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  handleStreamEvent: (event: AgentStreamEvent, sessionId: string) => void;
  loadNamespaces: () => Promise<void>;
  indexDocument: (namespace: string, content: string, source: string) => Promise<IndexDocumentResult>;
  deleteNamespace: (namespace: string) => Promise<boolean>;
  toggleSidebar: () => void;

  // 模型配置 actions
  loadProviders: () => Promise<void>;
  loadDefaultModel: () => Promise<void>;
  createOrUpdateProvider: (config: ProviderConfig) => Promise<void>;
  removeProvider: (id: string) => Promise<void>;
  setDefaultModel: (selection: ModelSelection) => Promise<void>;
  setSelectedModelForSession: (sessionId: string, selection: ModelSelection | null) => void;
  setPendingSessionModel: (selection: ModelSelection | null) => void;

  // 记忆开关 actions
  loadMemoryEnabled: () => Promise<void>;
  setMemoryEnabled: (enabled: boolean) => Promise<void>;

  // 按 session 读取的便捷 getter
  getSessionMessages: (sessionId: string) => ChatMessage[];
  getSessionStreamingState: (sessionId: string) => StreamingState;
  getSessionUnreadCount: (sessionId: string) => number;
  getSelectedModelForSession: (sessionId: string | null) => ModelSelection | null;
}

interface StreamEventContext {
  get: () => ChatState;
  set: (fn: (state: ChatState) => Partial<ChatState>) => void;
  isBackground: boolean;
}

type StreamEventHandlerMap = {
  [K in AgentStreamEvent['type']]?: (
    event: Extract<AgentStreamEvent, { type: K }>,
    sessionId: string,
    ctx: StreamEventContext,
  ) => void;
};

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

const streamEventHandlers: StreamEventHandlerMap = {
  user_message: (event, sessionId, { set }) => {
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
  },

  reason_delta: (event, sessionId, { set }) => {
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
  },

  assistant_delta: (event, sessionId, { set }) => {
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
  },

  assistant_done: (event, sessionId, { set, isBackground }) => {
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
  },

  tool_call_start: (event, sessionId, { set }) => {
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
  },

  tool_call_end: (event, sessionId, { set }) => {
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
  },

  tool_call_delta: (event, sessionId, { set }) => {
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
  },

  reason_done: () => {
    // reasoning 内容已通过 reason_delta 流式累积到消息 content 中，
    // 这里不需要额外更新；metadata.reason 会在 assistant_delta 切换时写入。
  },
};

export const useChatStore = create<ChatState>((set, get) => ({
  sessions: [],
  currentSessionId: null,
  messagesBySession: {},
  streamingBySession: {},
  unreadBySession: {},
  namespaces: [],
  providers: [],
  defaultModel: null,
  memoryEnabled: true,
  selectedModelBySession: {},
  pendingSessionModel: null,
  isSidebarCollapsed: false,

  getSessionMessages: (sessionId) => getMessages(get(), sessionId),
  getSessionStreamingState: (sessionId) => getStreamingState(get(), sessionId),
  getSessionUnreadCount: (sessionId) => get().unreadBySession[sessionId] ?? 0,
  getSelectedModelForSession: (sessionId) =>
    sessionId
      ? (get().selectedModelBySession[sessionId] ?? get().defaultModel)
      : (get().pendingSessionModel ?? get().defaultModel),

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

  startNewChat: () => {
    set((state) => {
      const { ["__pending__"]: _pendingMessages, ...restMessages } = state.messagesBySession;
      const { ["__pending__"]: _pendingStreaming, ...restStreaming } = state.streamingBySession;
      return {
        currentSessionId: null,
        pendingSessionModel: null,
        messagesBySession: restMessages,
        streamingBySession: restStreaming,
      };
    });
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
      const { [id]: _removedModel, ...restSelectedModels } = state.selectedModelBySession;

      return {
        sessions: remaining,
        currentSessionId: nextSessionId,
        messagesBySession: restMessages,
        streamingBySession: restStreaming,
        unreadBySession: restUnread,
        selectedModelBySession: restSelectedModels,
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
    const initialSessionId = get().currentSessionId;
    // 新会话模式下使用临时 key 缓存流式状态，待后端返回真实 session_id 后迁移。
    const tempSessionId = initialSessionId ?? "__pending__";
    const modelSelection = get().getSelectedModelForSession(initialSessionId);

    set((state) => ({
      streamingBySession: {
        ...state.streamingBySession,
        [tempSessionId]: { isStreaming: true, streamingMessageId: null },
      },
    }));

    // 流式回调通过闭包变量读取当前生效的 session id；创建新会话时会从 temp 切换到真实 id。
    let activeSessionId = tempSessionId;

    try {
      const returnedSessionId = await chatCompletionStream(
        initialSessionId,
        content,
        modelSelection,
        (event) => {
          get().handleStreamEvent(event, activeSessionId);
        },
      );

      // 后端创建了新会话：把临时 key 下的消息/流式状态迁移到真实 session。
      if (returnedSessionId !== initialSessionId) {
        activeSessionId = returnedSessionId;
        set((state) => {
          const pendingMessages = state.messagesBySession[tempSessionId] ?? [];
          const pendingStreaming = state.streamingBySession[tempSessionId] ?? {
            isStreaming: false,
            streamingMessageId: null,
          };
          const pendingModel = state.pendingSessionModel;

          const { [tempSessionId]: _removedMessages, ...restMessages } =
            state.messagesBySession;
          const { [tempSessionId]: _removedStreaming, ...restStreaming } =
            state.streamingBySession;

          const firstUserMessage = pendingMessages.find((m) => m.role === "user");
          const newSession: SessionSummary = {
            id: returnedSessionId,
            title: firstUserMessage
              ? formatTitle(firstUserMessage.content)
              : "新会话",
            updated_at: Math.floor(Date.now() / 1000),
          };

          return {
            currentSessionId: returnedSessionId,
            sessions: [newSession, ...state.sessions],
            messagesBySession: {
              ...restMessages,
              [returnedSessionId]: pendingMessages,
            },
            streamingBySession: {
              ...restStreaming,
              [returnedSessionId]: pendingStreaming,
            },
            selectedModelBySession: pendingModel
              ? {
                  ...state.selectedModelBySession,
                  [returnedSessionId]: pendingModel,
                }
              : state.selectedModelBySession,
            pendingSessionModel: null,
          };
        });
      }
    } finally {
      set((state) => ({
        streamingBySession: {
          ...state.streamingBySession,
          [activeSessionId]: { isStreaming: false, streamingMessageId: null },
        },
      }));
      // 刷新会话列表，使后台完成的会话排序与标题保持最新。
      await get().loadSessions();
    }
  },

  handleStreamEvent: (event, sessionId) => {
    const { currentSessionId } = get();
    // 当前未选中任何会话时（新会话刚发送第一条消息），所有事件都属于当前前台流式。
    const isBackground = currentSessionId !== null && sessionId !== currentSessionId;
    const handler = streamEventHandlers[event.type];
    if (handler) {
      handler(event as never, sessionId, { get, set, isBackground });
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

  toggleSidebar: () => {
    set((state) => ({ isSidebarCollapsed: !state.isSidebarCollapsed }));
  },

  loadProviders: async () => {
    const providers = await listProviders();
    set({ providers });
  },

  loadDefaultModel: async () => {
    const defaultModel = await getDefaultModel();
    set({ defaultModel });
  },

  createOrUpdateProvider: async (config) => {
    const providers = await saveProvider(config);
    set({ providers });
  },

  removeProvider: async (id) => {
    const providers = await deleteProvider(id);
    set({ providers });
  },

  setDefaultModel: async (selection) => {
    await setDefaultModel(selection);
    set({ defaultModel: selection });
  },

  loadMemoryEnabled: async () => {
    const memoryEnabled = await getMemoryEnabled();
    set({ memoryEnabled });
  },

  setMemoryEnabled: async (enabled) => {
    await setMemoryEnabled(enabled);
    set({ memoryEnabled: enabled });
  },

  setSelectedModelForSession: (sessionId, selection) => {
    set((state) => {
      if (!selection) {
        const { [sessionId]: _removed, ...rest } = state.selectedModelBySession;
        return { selectedModelBySession: rest };
      }
      return {
        selectedModelBySession: {
          ...state.selectedModelBySession,
          [sessionId]: selection,
        },
      };
    });
  },

  setPendingSessionModel: (selection) => {
    set({ pendingSessionModel: selection });
  },
}));
