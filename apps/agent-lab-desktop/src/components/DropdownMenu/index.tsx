import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

interface DropdownContextValue {
  isOpen: boolean;
  setIsOpen: (open: boolean) => void;
  toggle: () => void;
}

const DropdownContext = createContext<DropdownContextValue | null>(null);

function useDropdown() {
  const ctx = useContext(DropdownContext);
  if (!ctx) {
    throw new Error("DropdownMenu 组件必须包裹在 Dropdown 中使用");
  }
  return ctx;
}

interface DropdownProps {
  children: ReactNode;
  defaultOpen?: boolean;
}

export function Dropdown({ children, defaultOpen = false }: DropdownProps) {
  const [isOpen, setIsOpen] = useState(defaultOpen);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isOpen) return;

    const handleClickOutside = (event: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsOpen(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleEscape);

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [isOpen]);

  return (
    <DropdownContext.Provider
      value={{
        isOpen,
        setIsOpen,
        toggle: () => setIsOpen((prev) => !prev),
      }}
    >
      <div ref={containerRef} className="relative inline-block">
        {children}
      </div>
    </DropdownContext.Provider>
  );
}

interface DropdownTriggerProps {
  children: ReactNode;
  disabled?: boolean;
  className?: string;
  fullWidth?: boolean;
}

export function DropdownTrigger({
  children,
  disabled,
  className = "",
  fullWidth = false,
}: DropdownTriggerProps) {
  const { toggle, isOpen } = useDropdown();

  return (
    <button
      type="button"
      onClick={toggle}
      disabled={disabled}
      aria-expanded={isOpen}
      aria-haspopup="menu"
      className={`flex items-center gap-1 text-xs text-stone hover:text-ink transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
        fullWidth ? "w-full justify-between" : ""
      } ${className}`}
    >
      <span className={`truncate ${fullWidth ? "max-w-none" : "max-w-[160px]"}`}>
        {children}
      </span>
      <svg
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        className={`transition-transform duration-200 ${isOpen ? "rotate-180" : ""}`}
      >
        <polyline points="6 9 12 15 18 9" />
      </svg>
    </button>
  );
}

interface DropdownMenuProps {
  children: ReactNode;
  className?: string;
  placement?: "top" | "bottom";
  align?: "left" | "right";
}

export function DropdownMenu({
  children,
  className = "",
  placement = "bottom",
  align = "left",
}: DropdownMenuProps) {
  const { isOpen } = useDropdown();
  if (!isOpen) return null;

  const placementClass =
    placement === "top" ? "bottom-full mb-1" : "top-full mt-1";
  const alignClass = align === "right" ? "right-0" : "left-0";

  return (
    <div
      role="menu"
      className={`absolute ${placementClass} ${alignClass} min-w-[200px] max-w-sm bg-paper border border-mist rounded-sm shadow-card py-1 z-50 ${className}`}
    >
      {children}
    </div>
  );
}

interface DropdownMenuGroupProps {
  title: string;
  children: ReactNode;
}

export function DropdownMenuGroup({ title, children }: DropdownMenuGroupProps) {
  return (
    <div role="group" aria-label={title} className="border-t border-mist mt-1 pt-1">
      <div className="px-3 py-1 text-xs text-stone">{title}</div>
      {children}
    </div>
  );
}

interface DropdownMenuItemProps {
  children: ReactNode;
  onClick?: () => void;
  active?: boolean;
  disabled?: boolean;
}

export function DropdownMenuItem({
  children,
  onClick,
  active,
  disabled,
}: DropdownMenuItemProps) {
  const { setIsOpen } = useDropdown();

  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={() => {
        if (disabled) return;
        onClick?.();
        setIsOpen(false);
      }}
      className={`w-full text-left px-3 py-2 text-sm text-ink truncate transition-all border-l-3 ${
        active
          ? "bg-paper-dark border-moss"
          : "border-transparent hover:bg-paper-dark/50 disabled:hover:bg-transparent"
      } disabled:opacity-50 disabled:cursor-not-allowed`}
    >
      {children}
    </button>
  );
}

/* 便捷封装：类似原生 <select> 的受控下拉选择器 */
export interface DropdownOption<T = string> {
  value: T;
  label: string;
  disabled?: boolean;
}

interface DropdownSelectProps<T = string> {
  value: T;
  options: DropdownOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
  placeholder?: string;
  placement?: "top" | "bottom";
  align?: "left" | "right";
  className?: string;
  fullWidth?: boolean;
}

export function DropdownSelect<T extends string | number>({
  value,
  options,
  onChange,
  disabled,
  placeholder = "请选择",
  placement = "bottom",
  align = "left",
  className = "",
  fullWidth = false,
}: DropdownSelectProps<T>) {
  const selected = options.find((o) => o.value === value);

  return (
    <Dropdown>
      <DropdownTrigger
        disabled={disabled}
        className={className}
        fullWidth={fullWidth}
      >
        {selected?.label ?? placeholder}
      </DropdownTrigger>
      <DropdownMenu placement={placement} align={align}>
        {options.map((option) => (
          <DropdownMenuItem
            key={option.value}
            active={option.value === value}
            disabled={option.disabled}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenu>
    </Dropdown>
  );
}
