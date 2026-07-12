import { useCallback, useEffect, useRef, useState } from "react";
import {
  deleteTagAtBoundary,
  insertTag,
  measureItemsHeight,
  parseItemsFromHtml,
  serializeItems,
} from "./utils";

export interface UseRichEditorReturn {
  editorRef: React.RefObject<HTMLDivElement | null>;
  isComposing: boolean;
  setIsComposing: (value: boolean) => void;
  computedHeight: number;
  scheduleMeasureHeight: () => void;
  insertTag: (namespace: string, anchor: number) => void;
  deleteTagAtBoundary: () => boolean;
  clear: () => void;
  getItems: () => ReturnType<typeof parseItemsFromHtml>;
  serializeItems: () => string;
}

export function useRichEditor(): UseRichEditorReturn {
  const editorRef = useRef<HTMLDivElement>(null);
  const measureRafRef = useRef<number | null>(null);
  const [isComposing, setIsComposing] = useState(false);
  const [computedHeight, setComputedHeight] = useState(48);

  // 用 requestAnimationFrame 批量处理高度测量：连续输入事件会合并到下一帧统一计算，
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

  useEffect(() => {
    const editor = editorRef.current;
    if (!editor) return;

    scheduleMeasureHeight();

    const observer = new ResizeObserver(() => scheduleMeasureHeight());
    observer.observe(editor);
    return () => {
      observer.disconnect();
      if (measureRafRef.current !== null) {
        cancelAnimationFrame(measureRafRef.current);
      }
    };
  }, [scheduleMeasureHeight]);

  const insertTagCallback = useCallback((namespace: string, anchor: number) => {
    const editor = editorRef.current;
    if (!editor) return;
    insertTag(editor, namespace, anchor);
  }, []);

  const deleteTagAtBoundaryCallback = useCallback((): boolean => {
    const editor = editorRef.current;
    if (!editor) return false;
    return deleteTagAtBoundary(editor);
  }, []);

  const clear = useCallback(() => {
    const editor = editorRef.current;
    if (editor) {
      editor.innerHTML = "";
    }
    setComputedHeight(48);
  }, []);

  const getItems = useCallback(() => {
    const editor = editorRef.current;
    if (!editor) return [];
    return parseItemsFromHtml(editor);
  }, []);

  const serializeItemsCallback = useCallback(() => {
    return serializeItems(getItems());
  }, [getItems]);

  return {
    editorRef,
    isComposing,
    setIsComposing,
    computedHeight,
    scheduleMeasureHeight,
    insertTag: insertTagCallback,
    deleteTagAtBoundary: deleteTagAtBoundaryCallback,
    clear,
    getItems,
    serializeItems: serializeItemsCallback,
  };
}
