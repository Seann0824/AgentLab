import { useCallback, useState } from "react";

export interface UseExpandableReturn {
  isExpanded: boolean;
  toggle: () => void;
  setIsExpanded: (value: boolean) => void;
}

export function useExpandable(initial = false): UseExpandableReturn {
  const [isExpanded, setIsExpanded] = useState(initial);
  const toggle = useCallback(() => setIsExpanded((v) => !v), []);
  return { isExpanded, toggle, setIsExpanded };
}
