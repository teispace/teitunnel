import { type KeyboardEvent, type PointerEvent, useRef } from "react";
import { cn } from "@/lib/cn";

interface ResizeHandleProps {
  /** Current size of the pane being resized, in px. */
  size: number;
  min: number;
  max: number;
  onResize: (size: number) => void;
  /** Which edge of the pane the handle sits on. Dragging away from the pane grows it. */
  edge: "right" | "left";
  label: string;
  className?: string;
}

const KEY_STEP = 10;

/**
 * A pane divider: an 8 px hit area around an invisible or hairline edge. Direct
 * manipulation, so no animation; arrow keys resize for keyboard users.
 */
export function ResizeHandle({
  size,
  min,
  max,
  onResize,
  edge,
  label,
  className,
}: ResizeHandleProps) {
  const start = useRef<{ x: number; size: number } | null>(null);
  const clamp = (value: number) => Math.min(max, Math.max(min, Math.round(value)));
  const direction = edge === "right" ? 1 : -1;

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    start.current = { x: event.clientX, size };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!start.current) return;
    onResize(clamp(start.current.size + (event.clientX - start.current.x) * direction));
  };
  const onPointerUp = () => {
    start.current = null;
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const delta = event.key === "ArrowRight" ? KEY_STEP : event.key === "ArrowLeft" ? -KEY_STEP : 0;
    if (delta === 0) return;
    event.preventDefault();
    onResize(clamp(size + delta * direction));
  };

  return (
    // biome-ignore lint/a11y/useSemanticElements: a focusable separator is the ARIA pattern for splitters
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={size}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onKeyDown={onKeyDown}
      className={cn(
        "absolute inset-y-0 z-20 w-2 touch-none outline-none [&,&_*]:cursor-col-resize",
        edge === "right" ? "-right-1" : "-left-1",
        className,
      )}
    />
  );
}
