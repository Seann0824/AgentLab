import { create } from "zustand";
import {
  chatCompletionStream,
  createChatSession,
  deleteChatSession,
  getChatHistory,
  listChatSessions,
  renameChatSession,
} from "../api/chatApi";
import type { AgentStreamEvent, ChatMessage, SessionSummary } from "../types/chat";

interface ChatState {
  sessions: SessionSummary[];
  currentSessionId: string | null;
  messages: ChatMessage[];
  isStreaming: boolean;
  streamingMessageId: string | null;

  // actions
  loadSessions: () => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  createSession: () => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  handleStreamEvent: (event: AgentStreamEvent) => void;
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

export const useChatStore = create<ChatState>((set, get) => ({
  sessions: [],
  currentSessionId: null,
  messages: [],
  isStreaming: false,
  streamingMessageId: null,

  loadSessions: async () => {
    const sessions = await listChatSessions();
    set({ sessions });
  },

  selectSession: async (id) => {
    const messages = await getChatHistory(id);
    set({
      currentSessionId: id,
      messages,
      isStreaming: false,
      streamingMessageId: null,
    });
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
      messages: [],
      isStreaming: false,
      streamingMessageId: null,
    }));
  },

  deleteSession: async (id) => {
    const ok = await deleteChatSession(id);
    if (!ok) return;

    set((state) => {
      const remaining = state.sessions.filter((s) => s.id !== id);
      let nextSessionId = state.currentSessionId;
      let nextMessages: ChatMessage[] = state.messages;

      if (state.currentSessionId === id) {
        nextSessionId = remaining[0]?.id ?? null;
        nextMessages = [];
      }

      return { sessions: remaining, currentSessionId: nextSessionId, messages: nextMessages };
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
    const { currentSessionId } = get();
    set({ isStreaming: true, streamingMessageId: null });

    const returnedSessionId = await chatCompletionStream(
      currentSessionId,
      content,
      (event) => {
        get().handleStreamEvent(event);
      },
    );

    // 如果之前没有会话，后端会新建一个；需要同步到前端状态。
    if (!currentSessionId && returnedSessionId) {
      set((state) => {
        const exists = state.sessions.some((s) => s.id === returnedSessionId);
        const sessions = exists
          ? state.sessions
          : [
              {
                id: returnedSessionId,
                title: formatTitle(content),
                updated_at: Math.floor(Date.now() / 1000),
              },
              ...state.sessions,
            ];
        return {
          currentSessionId: returnedSessionId,
          sessions,
        };
      });
    }

    set({ isStreaming: false, streamingMessageId: null });
  },

  handleStreamEvent: (event) => {
    const { currentSessionId } = get();

    switch (event.type) {
      case "user_message": {
        set((state) => ({
          messages: [...state.messages, event.message],
          sessions: currentSessionId
            ? updateSessionTitle(
                state.sessions,
                currentSessionId,
                formatTitle(event.message.content),
              )
            : state.sessions,
        }));
        break;
      }

      case "reason_delta": {
        set((state) => {
          const targetId = event.message_id;
          const exists = state.messages.some((m) => m.id === targetId);

          if (!exists) {
            const newMessage: ChatMessage = {
              id: targetId,
              role: "assistant",
              content: event.delta,
              timestamp: Math.floor(Date.now() / 1000),
              metadata: { isReasoning: true },
            };
            return {
              messages: [...state.messages, newMessage],
              streamingMessageId: targetId,
            };
          }

          return {
            messages: state.messages.map((m) =>
              m.id === targetId ? { ...m, content: m.content + event.delta } : m,
            ),
            streamingMessageId: targetId,
          };
        });
        break;
      }

      case "assistant_delta": {
        set((state) => {
          const targetId = event.message_id;
          const existing = state.messages.find((m) => m.id === targetId);

          if (!existing) {
            const newMessage: ChatMessage = {
              id: targetId,
              role: "assistant",
              content: event.delta,
              timestamp: Math.floor(Date.now() / 1000),
            };
            return {
              messages: [...state.messages, newMessage],
              streamingMessageId: targetId,
            };
          }

          // 如果前一段是 reasoning，把 reasoning 内容移到 metadata.reason，
          // 然后开始写入正式回答内容。
          if (existing.metadata?.isReasoning) {
            const reasoningContent = existing.content;
            return {
              messages: state.messages.map((m) =>
                m.id === targetId
                  ? {
                      ...m,
                      content: event.delta,
                      metadata: {
                        ...m.metadata,
                        reason: reasoningContent,
                        isReasoning: false,
                      },
                    }
                  : m,
              ),
              streamingMessageId: targetId,
            };
          }

          return {
            messages: state.messages.map((m) =>
              m.id === targetId ? { ...m, content: m.content + event.delta } : m,
            ),
            streamingMessageId: targetId,
          };
        });
        break;
      }

      case "assistant_done": {
        set((state) => {
          const idx = state.messages.findIndex((m) => m.id === event.message.id);
          if (idx >= 0) {
            const next = [...state.messages];
            const existing = next[idx];
            // 合并后端 metadata 与前端流式过程中积累的 metadata（主要是 reason）。
            next[idx] = {
              ...event.message,
              metadata: {
                ...(existing.metadata ?? {}),
                ...(event.message.metadata ?? {}),
                isReasoning: false,
              },
            };
            return { messages: next, streamingMessageId: null };
          }

          // 兜底：id 不匹配时直接追加（正常不应发生）。
          return {
            messages: [...state.messages, event.message],
            streamingMessageId: null,
          };
        });
        break;
      }

      case "tool_call_start": {
        set((state) => {
          const exists = state.messages.some(
            (m) => m.tool_call_id === event.tool_call_id,
          );
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
          return { messages: [...state.messages, toolMessage] };
        });
        break;
      }

      case "tool_call_end": {
        set((state) => ({
          messages: state.messages.map((m) =>
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
        }));
        break;
      }

      case "tool_call_delta": {
        set((state) => ({
          messages: state.messages.map((m) =>
            m.tool_call_id === event.tool_call_id
              ? { ...m, content: m.content + event.delta }
              : m,
          ),
        }));
        break;
      }

      case "reason_done": {
        // reasoning 内容已通过 reason_delta 流式累积到消息 content 中，
        // 这里不需要额外更新；metadata.reason 会在 assistant_delta 切换时写入。
        break;
      }
    }
  },
}));
