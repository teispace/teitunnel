import { type ComponentProps, useState } from "react";
import { cn } from "@/lib/cn";

interface ScrollAreaProps extends ComponentProps<"div"> {
  /** Called when content scrolls under (or back from under) the top edge. */
  onScrolledChange?: (scrolled: boolean) => void;
}

/**
 * A native overlay-scrollbar region (WKWebView draws system scrollbars). Reports when
 * content has scrolled under the toolbar so it can show its separator, as AppKit does.
 */
export function ScrollArea({ className, onScrolledChange, onScroll, ...props }: ScrollAreaProps) {
  const [scrolled, setScrolled] = useState(false);
  return (
    <div
      className={cn("min-h-0 flex-1 overflow-y-auto overscroll-contain", className)}
      onScroll={(event) => {
        const next = event.currentTarget.scrollTop > 0;
        if (next !== scrolled) {
          setScrolled(next);
          onScrolledChange?.(next);
        }
        onScroll?.(event);
      }}
      {...props}
    />
  );
}
