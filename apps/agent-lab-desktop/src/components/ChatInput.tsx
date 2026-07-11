import { useEffect, useRef, useState } from "react";
import { useChatStore } from "../store/chatStore";

const TAG_CLASS = "namespace-tag";

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
  if (
    charBeforeTrigger !== null &&
    charBeforeTrigger !== " " &&
    charBeforeTrigger !== "\n"
  ) {
    return null;
  }

  return { anchor: triggerIndex, query };
}

function createNamespaceTag(namespace: string): HTMLSpanElement {
  const span = document.createElement("span");
  span.className = TAG_CLASS;
  span.contentEditable = "false";
  span.dataset.namespace = namespace;
  span.textContent = `@[${namespace}]`;
  return span;
}

function getPlainText(element: HTMLDivElement): string {
  return element.innerText || "";
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

function setCaretAfter(node: Node) {
  const selection = window.getSelection();
  if (!selection) return;
  const range = document.createRange();
  range.setStartAfter(node);
  range.collapse(true);
  selection.removeAllRanges();
  selection.addRange(range);
}

function insertTag(editor: HTMLDivElement, namespace: string, anchor: number) {
  const text = getPlainText(editor);
  const triggerInfo = findTriggerInfo(text, anchor + 1, "$");
  if (!triggerInfo) return;

  const { anchor: triggerIndex, query } = triggerInfo;
  const beforeText = text.slice(0, triggerIndex);
  const afterText = text.slice(triggerIndex + 1 + query.length);

  editor.innerHTML = "";

  if (beforeText) {
    editor.appendChild(document.createTextNode(beforeText));
  }

  const tag = createNamespaceTag(namespace);
  editor.appendChild(tag);
  editor.appendChild(document.createTextNode(` ${afterText}`));

  setCaretAfter(tag);
}

function deleteTagAtBoundary(): boolean {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return false;

  const range = selection.getRangeAt(0);
  if (!range.collapsed) return false;

  const node = range.startContainer;
  const offset = range.startOffset;

  // Backspace 紧贴 tag 后面：删除整个 tag
  if (
    offset > 0 &&
    node.childNodes[offset - 1] instanceof HTMLSpanElement
  ) {
    const tag = node.childNodes[offset - 1] as HTMLSpanElement;
    if (tag.classList.contains(TAG_CLASS)) {
      tag.remove();
      return true;
    }
  }

  // 光标在文本节点末尾，下一个是 tag
  if (node.nodeType === Node.TEXT_NODE) {
    const next = node.nextSibling;
    if (next instanceof HTMLSpanElement && next.classList.contains(TAG_CLASS)) {
      next.remove();
      return true;
    }
  }

  return false;
}

export function ChatInput() {
  const editorRef = useRef<HTMLDivElement>(null);
  const [showMenu, setShowMenu] = useState(false);
  const [menuQuery, setMenuQuery] = useState("");
  const [menuIndex, setMenuIndex] = useState(0);
  const [menuAnchor, setMenuAnchor] = useState(0);

  const isStreaming = useChatStore((s) => s.isStreaming);
  const sendMessage = useChatStore((s) => s.sendMessage);
  const namespaces = useChatStore((s) => s.namespaces);
  const loadNamespaces = useChatStore((s) => s.loadNamespaces);

  useEffect(() => {
    loadNamespaces();
  }, [loadNamespaces]);

  const filteredNamespaces = namespaces
    .filter((ns) => ns.toLowerCase().includes(menuQuery.toLowerCase()))
    .slice(0, 8);

  const handleSend = async () => {
    const editor = editorRef.current;
    if (!editor || isStreaming) return;

    const text = getPlainText(editor).trim();
    if (!text) return;

    editor.innerHTML = "";
    setShowMenu(false);
    await sendMessage(text);
  };

  const updateMenuFromCursor = () => {
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
  };

  const insertNamespace = (namespace: string) => {
    const editor = editorRef.current;
    if (!editor) return;

    insertTag(editor, namespace, menuAnchor);
    setShowMenu(false);
    setMenuQuery("");
    editor.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (showMenu) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setMenuIndex((i) =>
          Math.min(i + 1, Math.max(filteredNamespaces.length - 1, 0)),
        );
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

    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
      return;
    }

    if ((e.key === "Backspace" || e.key === "Delete") && !e.shiftKey) {
      if (deleteTagAtBoundary()) {
        e.preventDefault();
      }
    }
  };

  const handleInput = () => {
    updateMenuFromCursor();
  };

  const handleSelect = () => {
    updateMenuFromCursor();
  };

  const handlePaste = (e: React.ClipboardEvent<HTMLDivElement>) => {
    e.preventDefault();
    const text = e.clipboardData.getData("text/plain");
    document.execCommand("insertText", false, text);
  };

  return (
    <div className="px-6 py-4 border-t border-mist bg-paper relative">
      {showMenu && filteredNamespaces.length > 0 && (
        <div className="absolute left-6 right-6 bottom-full mb-2 max-w-4xl mx-auto">
          <div className="bg-paper-dark border border-mist rounded-lg shadow-lg py-1 max-h-48 overflow-y-auto">
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
          </div>
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
          suppressContentEditableWarning
          data-placeholder="输入消息，Shift + Enter 换行，$ 选择知识库…"
          className="input-minimal flex-1 max-h-40 overflow-y-auto resize-none py-3 px-4 text-ink whitespace-pre-wrap"
          style={{ minHeight: "48px", outline: "none" }}
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
