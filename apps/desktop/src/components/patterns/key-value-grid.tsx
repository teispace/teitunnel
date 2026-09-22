import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface KeyValue {
  label: string;
  value: ReactNode;
  /** Monospace + selectable, for ids, hostnames and URLs. */
  mono?: boolean;
}

/** Label/value pairs, labels right-aligned like a Get Info window. */
export function KeyValueGrid({
  items,
  className,
}: {
  items: readonly KeyValue[];
  className?: string;
}) {
  return (
    <dl className={cn("grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5", className)}>
      {items.map((item) => (
        <div key={item.label} className="contents">
          <dt className="text-right text-callout text-secondary leading-4">{item.label}</dt>
          <dd
            className={cn(
              "selectable min-w-0 truncate",
              item.mono ? "font-mono text-mono" : "text-callout leading-4",
            )}
          >
            {item.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
