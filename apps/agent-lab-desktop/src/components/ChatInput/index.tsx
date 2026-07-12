import { useEffect } from "react";
import { useChatStore } from "../../store/chatStore";
import { ModelSelector } from "../ModelSelector";
import { ScrollContainer } from "../ScrollContainer";
import { useModelSelector } from "./useModelSelector";
import { useNamespaceMention } from "./useNamespaceMention";
import { useRichEditor } from "./useRichEditor";
import { useSendMessage } from "./useSendMessage";

export function ChatInput() {
  const rich = useRichEditor();
  const send = useSendMessage();
  const model = useModelSelector();

  const namespaces = useChatStore((s) => s.namespaces);
  const loadNamespaces = useChatStore((s) => s.loadNamespaces);
  const loadProviders = useChatStore((s) => s.loadProviders);
  const loadDefaultModel = useChatStore((s) => s.loadDefaultModel);

  useEffect(() => {
    loadNamespaces();
    loadProviders();
    loadDefaultModel();
  }, [loadNamespaces, loadProviders, loadDefaultModel]);

  const mention = useNamespaceMention({
    editorRef: rich.editorRef,
    namespaces,
    onSelect: (namespace) => {
      rich.insertTag(namespace, mention.menuAnchor);
      rich.scheduleMeasureHeight();
    },
  });

  const handleSend = async () => {
    const text = rich.serializeItems();
    if (!text.trim() || send.isStreaming) return;

    rich.clear();
    mention.closeMenu();
    await send.handleSend(text);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (mention.handleKeyDown(event)) return;

    if (event.key === "Enter" && !event.shiftKey && !rich.isComposing) {
      event.preventDefault();
      handleSend();
      return;
    }

    if (event.key === "Backspace" && !event.shiftKey) {
      if (rich.deleteTagAtBoundary()) {
        event.preventDefault();
        mention.updateMenu();
        rich.scheduleMeasureHeight();
      }
    }
  };

  const handleInput = () => {
    mention.updateMenu();
    rich.scheduleMeasureHeight();
  };

  const handleSelect = () => {
    mention.updateMenu();
  };

  const handlePaste = (event: React.ClipboardEvent<HTMLDivElement>) => {
    event.preventDefault();
    const text = event.clipboardData.getData("text/plain");
    document.execCommand("insertText", false, text);
    rich.scheduleMeasureHeight();
  };

  return (
    <div className="px-6 py-4 border-t border-mist bg-paper relative">
      {mention.showMenu && mention.filteredNamespaces.length > 0 && (
        <div className="absolute left-6 right-6 bottom-full mb-2 max-w-4xl mx-auto">
          <ScrollContainer className="bg-paper-dark border border-mist rounded-lg shadow-lg py-1 max-h-48">
            {mention.filteredNamespaces.map((ns, idx) => (
              <button
                key={ns}
                type="button"
                onClick={() => mention.selectNamespace(ns)}
                className={`w-full text-left px-3 py-2 text-sm ${
                  idx === mention.menuIndex
                    ? "bg-moss/10 text-moss"
                    : "text-ink hover:bg-paper"
                }`}
              >
                {ns}
              </button>
            ))}
          </ScrollContainer>
        </div>
      )}

      <div className="flex items-end gap-3 max-w-4xl mx-auto">
        <div className="relative flex-1">
          <div
            ref={rich.editorRef}
            contentEditable
            role="textbox"
            aria-multiline="true"
            onInput={handleInput}
            onKeyDown={handleKeyDown}
            onSelect={handleSelect}
            onPaste={handlePaste}
            onCompositionStart={() => rich.setIsComposing(true)}
            onCompositionEnd={() => rich.setIsComposing(false)}
            suppressContentEditableWarning
            data-placeholder="输入消息，Shift + Enter 换行，$ 选择知识库…"
            className="input-minimal custom-scrollbar w-full h-32 overflow-y-auto resize-none py-3 pl-4 pr-[140px] text-ink whitespace-pre-wrap"
            style={{ outline: "none" }}
          />
          <div className="absolute right-3 bottom-3 flex items-center gap-2">
            <ModelSelector
              groups={model.modelGroups}
              currentKey={model.currentModelKey}
              onChange={model.selectModel}
              disabled={model.disabled}
            />
            <button
              onClick={handleSend}
              disabled={send.isStreaming}
              aria-label={send.isStreaming ? "思考中" : "发送"}
              className="flex items-center justify-center w-8 h-8 bg-moss text-paper rounded-sm hover:bg-moss-dark transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <line x1="22" y1="2" x2="11" y2="13" />
                <polygon points="22 2 15 22 11 13 2 9 22 2" />
              </svg>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
