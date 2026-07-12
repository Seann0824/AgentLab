import {
  createContext,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

type AccordionType = "single" | "multiple";

interface AccordionContextValue {
  value: string[];
  toggle: (itemValue: string) => void;
}

const AccordionContext = createContext<AccordionContextValue | null>(null);

function useAccordion() {
  const ctx = useContext(AccordionContext);
  if (!ctx) {
    throw new Error("AccordionItem 必须包裹在 Accordion 中使用");
  }
  return ctx;
}

interface AccordionItemContextValue {
  value: string;
}

const AccordionItemContext = createContext<AccordionItemContextValue | null>(
  null,
);

function useAccordionItem() {
  const ctx = useContext(AccordionItemContext);
  if (!ctx) {
    throw new Error(
      "AccordionTrigger / AccordionContent 必须包裹在 AccordionItem 中使用",
    );
  }
  return ctx;
}

interface AccordionProps {
  children: ReactNode;
  type?: AccordionType;
  collapsible?: boolean;
  defaultValue?: string[];
  value?: string[];
  onValueChange?: (value: string[]) => void;
  className?: string;
}

export function Accordion({
  children,
  type = "single",
  collapsible = true,
  defaultValue,
  value: controlledValue,
  onValueChange,
  className = "",
}: AccordionProps) {
  const isControlled = controlledValue !== undefined;
  const [internalValue, setInternalValue] = useState<string[]>(
    defaultValue ?? [],
  );
  const value = isControlled ? controlledValue : internalValue;

  const setValue = (next: string[]) => {
    if (!isControlled) {
      setInternalValue(next);
    }
    onValueChange?.(next);
  };

  const toggle = (itemValue: string) => {
    if (type === "single") {
      if (value.includes(itemValue)) {
        if (collapsible) {
          setValue([]);
        }
      } else {
        setValue([itemValue]);
      }
    } else {
      if (value.includes(itemValue)) {
        setValue(value.filter((v) => v !== itemValue));
      } else {
        setValue([...value, itemValue]);
      }
    }
  };

  const ctx = useMemo(() => ({ value, toggle }), [value]);

  return (
    <AccordionContext.Provider value={ctx}>
      <div className={`space-y-2 ${className}`}>{children}</div>
    </AccordionContext.Provider>
  );
}

interface AccordionItemProps {
  children: ReactNode;
  value: string;
  className?: string;
}

export function AccordionItem({
  children,
  value,
  className = "",
}: AccordionItemProps) {
  const ctx = useMemo(() => ({ value }), [value]);

  return (
    <AccordionItemContext.Provider value={ctx}>
      <div
        className={`bg-paper border border-mist rounded-sm overflow-hidden ${className}`}
      >
        {children}
      </div>
    </AccordionItemContext.Provider>
  );
}

interface AccordionTriggerProps {
  children: ReactNode;
  className?: string;
}

export function AccordionTrigger({
  children,
  className = "",
}: AccordionTriggerProps) {
  const { value, toggle } = useAccordion();
  const { value: itemValue } = useAccordionItem();
  const isOpen = value.includes(itemValue);

  return (
    <button
      type="button"
      onClick={() => toggle(itemValue)}
      aria-expanded={isOpen}
      className={`w-full flex items-center justify-between gap-3 px-4 py-3 text-left transition-colors duration-200 ${
        isOpen
          ? "bg-paper-dark text-ink"
          : "text-ink hover:bg-paper-dark/50"
      } ${className}`}
    >
      <div className="flex-1 min-w-0">{children}</div>
      <svg
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        className={`flex-shrink-0 text-stone transition-transform duration-200 ${
          isOpen ? "rotate-180" : ""
        }`}
        aria-hidden="true"
      >
        <polyline points="6 9 12 15 18 9" />
      </svg>
    </button>
  );
}

interface AccordionContentProps {
  children: ReactNode;
  className?: string;
}

export function AccordionContent({
  children,
  className = "",
}: AccordionContentProps) {
  const { value } = useAccordion();
  const { value: itemValue } = useAccordionItem();
  const isOpen = value.includes(itemValue);

  return (
    <div
      className={`grid transition-[grid-template-rows] duration-200 ease-out ${
        isOpen ? "grid-rows-[1fr]" : "grid-rows-[0fr]"
      }`}
    >
      <div className={`overflow-hidden ${className}`}>{children}</div>
    </div>
  );
}
