import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/**
 * System Settings–style grouped form (measured on macOS 27): a bold section title,
 * then a 12 px-radius inset card whose rows are separated by inset hairlines.
 */
export function GroupedSection({
  title,
  footer,
  children,
  className,
}: {
  title?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("flex flex-col", className)}>
      {title ? <h3 className="px-2.5 pb-2.5 text-headline">{title}</h3> : null}
      <div className="rounded-card bg-surface-inset px-2.5">
        <div className="flex flex-col divide-y-(length:--hairline) divide-inset">{children}</div>
      </div>
      {footer ? <p className="px-2.5 pt-2 text-callout text-secondary">{footer}</p> : null}
    </section>
  );
}

export function GroupedRow({
  label,
  description,
  children,
  className,
}: {
  label: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex min-h-9 items-center gap-4 py-2", className)}>
      <div className="min-w-0 flex-1">
        <div className="text-body">{label}</div>
        {description ? (
          <div className="mt-0.5 text-callout text-secondary">{description}</div>
        ) : null}
      </div>
      {children ? <div className="flex shrink-0 items-center gap-2">{children}</div> : null}
    </div>
  );
}
