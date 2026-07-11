import { useEffect, useRef, useState, useCallback } from "react";
import { useChatStore } from "../store/chatStore";
import { ScrollContainer } from "./ScrollContainer";
import { prepareRichInline, measureRichInlineStats } from "@chenglou/pretext/rich-inline";

const TAG_CLASS = "namespace-tag";

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function createTagHtml(namespace: string): string {
  return `<span class="${TAG_CLASS}" contenteditable="false" data-namespace="${escapeHtml(
    namespace,
  )}">@[${escapeHtml(namespace)}]</span>`;
}

function getPlainText(element: HTMLDivElement): string {
  return element.innerText || "";
}

function findTriggerInfo(
  text: string,
  cursor: number,
  trigger: string,
): { anchor: number; query: string } | null {
  const textBeforeCursor = text.slice(0, cursor);
  const triggerIndex = textBeforeCursor.lastIndexOf(trigger);
  if (triggerIndex === -1) return null;

  const query = textBeforeCursor.slice(triggerIndex + trigger.length);
  if (query.includes(" ") || query.includes("\n")) return null;

  const charBeforeTrigger = triggerIndex > 0 ? textBeforeCursor[triggerIndex - 1] : null;
  if (charBeforeTrigger !== null && charBeforeTrigger !== " " && charBeforeTrigger !== "\n") {
    return null;
  }

  return { anchor: triggerIndex, query };
}

function getCaretOffset(editor: HTMLDivElement): number {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return 0;
  const range = selection.getRangeAt(0);
  const preCaretRange = range.cloneRange();
  preCaretRange.selectNodeContents(editor);
  preCaretRange.setEnd(range.endContainer, range.endOffset);
  return preCaretRange.toString().length;
}

function insertTag(editor: HTMLDivElement, namespace: string, anchor: number) {
  const text = getPlainText(editor);
  const info = findTriggerInfo(text, anchor + 1, "$");
  if (!info) return;

  const { anchor: triggerIndex, query } = info;
  const beforeText = text.slice(0, triggerIndex);
  const afterText = text.slice(triggerIndex + 1 + query.length);

  const beforeHtml = escapeHtml(beforeText).replace(/\n/g, "<br>");
  const afterHtml = escapeHtml(afterText ? ` ${afterText}` : "").replace(/\n/g, "<br>");

  editor.innerHTML = `${beforeHtml}${createTagHtml(namespace)}${afterHtml}`;

  // 光标放到 tag 后面
  const tag = editor.querySelector(`.${TAG_CLASS}[data-namespace="${CSS.escape(namespace)}"]`);
  if (tag) {
    const selection = window.getSelection();
    if (selection) {
      const range = document.createRange();
      range.setStartAfter(tag);
      range.collapse(true);
      selection.removeAllRanges();
      selection.addRange(range);
    }
  }
}

function deleteTagAtBoundary(editor: HTMLDivElement): boolean {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return false;

  const range = selection.getRangeAt(0);
  if (!range.collapsed) return false;

  const node = range.startContainer;
  const offset = range.startOffset;

  // 情况 1：光标在容器节点中，offset 指向 tag 之后
  if (node === editor && offset > 0) {
    const child = node.childNodes[offset - 1];
    if (child instanceof HTMLSpanElement && child.classList.contains(TAG_CLASS)) {
      child.remove();
      return true;
    }
  }

  // 情况 2：光标在文本节点开头，前一个兄弟是 tag
  if (node.nodeType === Node.TEXT_NODE && offset === 0) {
    const prev = node.previousSibling;
    if (prev instanceof HTMLSpanElement && prev.classList.contains(TAG_CLASS)) {
      prev.remove();
      return true;
    }
  }

  // 情况 3：光标在 tag 后面的文本节点中任意位置，前一个兄弟是 tag
  if (node.nodeType === Node.TEXT_NODE) {
    const prev = node.previousSibling;
    if (prev instanceof HTMLSpanElement && prev.classList.contains(TAG_CLASS) && offset === 0) {
      prev.remove();
      return true;
    }
  }

  return false;
}

type InlineItem =
  | { type: "text"; text: string }
  | { type: "tag"; namespace: string };

function parseItemsFromHtml(container: HTMLDivElement): InlineItem[] {
  const items: InlineItem[] = [];
  let currentText = "";

  function flushText() {
    if (currentText) {
      items.push({ type: "text", text: currentText });
      currentText = "";
    }
  }

  function walk(node: Node) {
    if (node.nodeType === Node.TEXT_NODE) {
      currentText += node.textContent ?? "";
      return;
    }

    if (node instanceof HTMLBRElement) {
      currentText += "\n";
      return;
    }

    if (node instanceof HTMLSpanElement && node.classList.contains(TAG_CLASS)) {
      flushText();
      const ns = node.dataset.namespace ?? node.textContent?.slice(2, -1) ?? "";
      if (ns) items.push({ type: "tag", namespace: ns });
      return;
    }

    if (node instanceof HTMLElement) {
      for (const child of node.childNodes) {
        walk(child);
      }
    }
  }

  for (const child of container.childNodes) {
    walk(child);
  }
  flushText();
  return items;
}

function serializeItems(items: InlineItem[]): string {
  return items
    .map((item) => (item.type === "text" ? item.text : `@[${item.namespace}]`))
    .join("");
}

function measureItemsHeight(items: InlineItem[], width: number): number {
  const richItems = items.map((item) =>
    item.type === "text"
      ? { text: item.text, font: "400 14px Inter, sans-serif" }
      : {
          text: `@[${item.namespace}]`,
          font: "500 12px Inter, sans-serif",
          break: "never" as const,
          extraWidth: 24,
        },
  );
  const prepared = prepareRichInline(richItems);
  const { lineCount } = measureRichInlineStats(prepared, Math.max(1, width));
  return lineCount * 22 + 24; // 24 = vertical padding
}

export function ChatInput() {
  const editorRef = useRef<HTMLDivElement>(null);
  const [showMenu, setShowMenu] = useState(false);
  const [menuQuery, setMenuQuery] = useState("");
  const [menuIndex, setMenuIndex] = useState(0);
  const [menuAnchor, setMenuAnchor] = useState(0);
  const [isComposing, setIsComposing] = useState(false);
  const [computedHeight, setComputedHeight] = useState(48);
  const measureRafRef = useRef<number | null>(null);

  const currentSessionId = useChatStore((s) => s.currentSessionId);
  const isStreaming = useChatStore((s) =>
    currentSessionId
      ? (s.streamingBySession[currentSessionId]?.isStreaming ?? false)
      : false,
  );
  const sendMessage = useChatStore((s) => s.sendMessage);
  const namespaces = useChatStore((s) => s.namespaces);
  const loadNamespaces = useChatStore((s) => s.loadNamespaces);

  useEffect(() => {
    loadNamespaces();
  }, [loadNamespaces]);

  useEffect(() => {
    const editor = editorRef.current;
    if (!editor) return;

    // 初始化时测量一次高度，并在宽度变化时通过 rAF 批量重测。
    scheduleMeasureHeight();

    const observer = new ResizeObserver(() => scheduleMeasureHeight());
    observer.observe(editor);
    return () => {
      observer.disconnect();
      if (measureRafRef.current !== null) {
        cancelAnimationFrame(measureRafRef.current);
      }
    };
  }, []);

  const filteredNamespaces = namespaces
    .filter((ns) => ns.toLowerCase().includes(menuQuery.toLowerCase()))
    .slice(0, 8);

  const updateMenu = useCallback(() => {
    const editor = editorRef.current;
    if (!editor) return;

    const text = getPlainText(editor);
    const cursor = getCaretOffset(editor);
    const info = findTriggerInfo(text, cursor, "$");

    if (!info) {
      setShowMenu(false);
      return;
    }

    setMenuAnchor(info.anchor);
    setMenuQuery(info.query);
    setMenuIndex(0);
    setShowMenu(true);
  }, []);

  // 用 requestAnimationFrame 批量处理高度测量：连续输入事件会被合并到下一帧统一计算，
  // 避免 pretext 的文本测量阻塞每一帧。
  const scheduleMeasureHeight = useCallback(() => {
    if (measureRafRef.current !== null) return;
    measureRafRef.current = requestAnimationFrame(() => {
      measureRafRef.current = null;
      const editor = editorRef.current;
      if (!editor) return;
      const items = parseItemsFromHtml(editor);
      const height = measureItemsHeight(items, editor.clientWidth);
      setComputedHeight((prev) => (prev === height ? prev : height));
    });
  }, []);

  const insertNamespace = (namespace: string) => {
    const editor = editorRef.current;
    if (!editor) return;

    insertTag(editor, namespace, menuAnchor);
    setShowMenu(false);
    setMenuQuery("");
    editor.focus();
    updateMenu();
    scheduleMeasureHeight();
  };

  const handleSend = async () => {
    const editor = editorRef.current;
    if (!editor || isStreaming) return;

    const items = parseItemsFromHtml(editor);
    const text = serializeItems(items).trim();
    if (!text) return;

    editor.innerHTML = "";
    setShowMenu(false);
    setComputedHeight(48);
    await sendMessage(text);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (showMenu) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setMenuIndex((i) => Math.min(i + 1, Math.max(filteredNamespaces.length - 1, 0)));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setMenuIndex((i) => Math.max(i - 1, 0));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        if (filteredNamespaces[menuIndex]) {
          insertNamespace(filteredNamespaces[menuIndex]);
        }
        return;
      }
      if (e.key === "Escape") {
        setShowMenu(false);
        return;
      }
    }

    if (e.key === "Enter" && !e.shiftKey && !isComposing) {
      e.preventDefault();
      handleSend();
      return;
    }

    if (e.key === "Backspace" && !e.shiftKey) {
      const editor = editorRef.current;
      if (editor && deleteTagAtBoundary(editor)) {
        e.preventDefault();
        updateMenu();
        scheduleMeasureHeight();
      }
    }
  };

  const handleInput = () => {
    updateMenu();
    scheduleMeasureHeight();
  };

  const handleSelect = () => {
    updateMenu();
  };

  const handlePaste = (e: React.ClipboardEvent<HTMLDivElement>) => {
    e.preventDefault();
    const text = e.clipboardData.getData("text/plain");
    document.execCommand("insertText", false, text);
    scheduleMeasureHeight();
  };

  return (
    <div className="px-6 py-4 border-t border-mist bg-paper relative">
      {showMenu && filteredNamespaces.length > 0 && (
        <div className="absolute left-6 right-6 bottom-full mb-2 max-w-4xl mx-auto">
          <ScrollContainer className="bg-paper-dark border border-mist rounded-lg shadow-lg py-1 max-h-48">
            {filteredNamespaces.map((ns, idx) => (
              <button
                key={ns}
                type="button"
                onClick={() => insertNamespace(ns)}
                className={`w-full text-left px-3 py-2 text-sm ${
                  idx === menuIndex
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
        <div
          ref={editorRef}
          contentEditable
          role="textbox"
          aria-multiline="true"
          onInput={handleInput}
          onKeyDown={handleKeyDown}
          onSelect={handleSelect}
          onPaste={handlePaste}
          onCompositionStart={() => setIsComposing(true)}
          onCompositionEnd={() => setIsComposing(false)}
          suppressContentEditableWarning
          data-placeholder="输入消息，Shift + Enter 换行，$ 选择知识库…"
          className="input-minimal custom-scrollbar flex-1 max-h-40 overflow-y-auto resize-none py-3 px-4 text-ink whitespace-pre-wrap"
          style={{
            minHeight: "48px",
            outline: "none",
            height: `${Math.min(Math.max(computedHeight, 48), 160)}px`,
          }}
        />
        <button
          onClick={handleSend}
          disabled={isStreaming}
          className="btn-moss px-6 py-3 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {isStreaming ? "思考中" : "发送"}
        </button>
      </div>
    </div>
  );
}
