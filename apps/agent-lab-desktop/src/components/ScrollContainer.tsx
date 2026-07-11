import { forwardRef, type ReactNode } from "react";

type ScrollDirection = "vertical" | "horizontal" | "both";

interface ScrollContainerProps {
  children: ReactNode;
  direction?: ScrollDirection;
  className?: string;
  style?: React.CSSProperties;
  onScroll?: React.UIEventHandler<HTMLDivElement>;
}

export const ScrollContainer = forwardRef<HTMLDivElement, ScrollContainerProps>(
  function ScrollContainer(
    { children, direction = "vertical", className = "", style, onScroll },
    ref,
  ) {
    const overflowClass = {
      vertical: "overflow-y-auto overflow-x-hidden",
      horizontal: "overflow-x-auto overflow-y-hidden",
      both: "overflow-auto",
    }[direction];

    return (
      <div
        ref={ref}
        onScroll={onScroll}
        className={`custom-scrollbar ${overflowClass} ${className}`}
        style={style}
      >
        {children}
      </div>
    );
  },
);
