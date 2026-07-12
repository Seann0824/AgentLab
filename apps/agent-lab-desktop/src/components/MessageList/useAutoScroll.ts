import { useCallback, useRef } from "react";

const BOTTOM_THRESHOLD = 35; // px

export interface UseAutoScrollReturn {
  scrollRef: React.RefObject<HTMLDivElement | null>;
  bottomRef: React.RefObject<HTMLDivElement | null>;
  handleScroll: () => void;
  scrollToBottom: () => void;
  tryScrollToBottom: () => void;
}

export function useAutoScroll(): UseAutoScrollReturn {
  const scrollRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);

  const scrollToBottom = useCallback(() => {
    requestAnimationFrame(() => {
      const bottomEl = bottomRef.current;
      if (bottomEl) {
        bottomEl.scrollIntoView({ block: "end", inline: "nearest" });
      } else {
        const el = scrollRef.current;
        if (el) {
          el.scrollTo({ top: el.scrollHeight, behavior: "auto" });
        }
      }
      isAtBottomRef.current = true;
    });
  }, []);

  const tryScrollToBottom = useCallback(() => {
    if (isAtBottomRef.current) {
      scrollToBottom();
    }
  }, [scrollToBottom]);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    isAtBottomRef.current = distanceFromBottom < BOTTOM_THRESHOLD;
  }, []);

  return {
    scrollRef,
    bottomRef,
    handleScroll,
    scrollToBottom,
    tryScrollToBottom,
  };
}
