import { ToggleGroup } from "radix-ui";
import { cn } from "@/lib/cn";

export interface Segment<T extends string> {
  value: T;
  label: string;
}

interface SegmentedControlProps<T extends string> {
  segments: readonly Segment<T>[];
  value: T;
  onValueChange: (value: T) => void;
  label: string;
  size?: "sm" | "md";
  className?: string;
}

/**
 * NSSegmentedControl: equal-width segments with a thumb that slides on the snappy
 * spring. Arrow keys move focus (roving tab index); Space/Enter select.
 */
export function SegmentedControl<T extends string>({
  segments,
  value,
  onValueChange,
  label,
  size = "md",
  className,
}: SegmentedControlProps<T>) {
  const index = Math.max(
    0,
    segments.findIndex((segment) => segment.value === value),
  );
  return (
    <ToggleGroup.Root
      type="single"
      aria-label={label}
      value={value}
      onValueChange={(next) => {
        // Radix allows deselecting; a segmented control always has a selection.
        if (next) onValueChange(next as T);
      }}
      className={cn(
        "relative inline-grid auto-cols-fr grid-flow-col rounded-full bg-surface-control p-0.5",
        size === "sm" ? "h-5 text-callout" : "h-6 text-body",
        className,
      )}
    >
      <span
        aria-hidden
        className="absolute inset-y-0.5 left-0.5 rounded-full bg-surface-thumb shadow-control transition-transform transition-snappy"
        style={{
          width: `calc((100% - 4px) / ${segments.length})`,
          transform: `translateX(${index * 100}%)`,
        }}
      />
      {segments.map((segment) => (
        <ToggleGroup.Item
          key={segment.value}
          value={segment.value}
          className="relative z-10 rounded-full px-3 text-primary outline-offset-0 data-[state=off]:text-secondary"
        >
          {segment.label}
        </ToggleGroup.Item>
      ))}
    </ToggleGroup.Root>
  );
}
