import { prepareRichInline, measureRichInlineStats } from "@chenglou/pretext/rich-inline";

const TAG_CLASS = "namespace-tag";

export type InlineItem =
  | { type: "text"; text: string }
  | { type: "tag"; namespace: string };

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function createTagHtml(namespace: string): string {
  return `<span class="${TAG_CLASS}" contenteditable="false" data-namespace="${escapeHtml(
    namespace,
  )}">@[${escapeHtml(namespace)}]</span>`;
}

export function getPlainText(element: HTMLDivElement): string {
  return element.innerText || "";
}

export function findTriggerInfo(
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

export function getCaretOffset(editor: HTMLDivElement): number {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return 0;
  const range = selection.getRangeAt(0);
  const preCaretRange = range.cloneRange();
  preCaretRange.selectNodeContents(editor);
  preCaretRange.setEnd(range.endContainer, range.endOffset);
  return preCaretRange.toString().length;
}

export function insertTag(editor: HTMLDivElement, namespace: string, anchor: number) {
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

export function deleteTagAtBoundary(editor: HTMLDivElement): boolean {
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

export function parseItemsFromHtml(container: HTMLDivElement): InlineItem[] {
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

export function serializeItems(items: InlineItem[]): string {
  return items
    .map((item) => (item.type === "text" ? item.text : `@[${item.namespace}]`))
    .join("");
}

export function measureItemsHeight(items: InlineItem[], width: number): number {
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
