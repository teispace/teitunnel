import { Link, type LinkProps } from "@tanstack/react-router";
import { ChevronRight, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/cn";

/**
 * Dashboard building blocks: cards on a grid that fills the window (no narrow centred
 * column on a large display). Grids use container queries, so they follow the content
 * pane's width, not the window's (the sidebar can be shown or hidden).
 */

export type Tone = "neutral" | "healthy" | "warning" | "error" | "accent";

const toneText: Record<Tone, string> = {
  neutral: "text-primary",
  healthy: "text-healthy",
  warning: "text-warning",
  error: "text-error",
  accent: "text-accent",
};

const toneIcon: Record<Tone, string> = {
  neutral: "bg-surface-control text-secondary",
  healthy: "bg-healthy/15 text-healthy",
  warning: "bg-warning/15 text-warning",
  error: "bg-error/15 text-error",
  accent: "bg-accent/15 text-accent",
};

/** The page's scrolling body: full width up to a comfortable maximum. */
export function DashboardPage({ children }: { children: ReactNode }) {
  return (
    <div className="@container flex-1 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-[1680px] flex-col gap-4 px-6 pt-2 pb-10">
        {children}
      </div>
    </div>
  );
}

/** A card: a titled surface. `to` makes the title row a link ("Show All"). */
export function Card({
  title,
  detail,
  action,
  children,
  className,
  bodyClassName,
}: {
  title?: ReactNode;
  detail?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section
      className={cn(
        "flex min-w-0 flex-col rounded-card border-hairline border-inset bg-surface-inset",
        className,
      )}
    >
      {title ? (
        <header className="flex items-start justify-between gap-3 px-4 pt-3.5 pb-2">
          <div className="min-w-0">
            <h2 className="truncate text-headline">{title}</h2>
            {detail ? <p className="truncate text-callout text-secondary">{detail}</p> : null}
          </div>
          {action ? <div className="flex shrink-0 items-center gap-2">{action}</div> : null}
        </header>
      ) : null}
      <div className={cn("min-w-0 flex-1 px-4 pb-3.5", !title && "pt-3.5", bodyClassName)}>
        {children}
      </div>
    </section>
  );
}

/** A card's "Show All"-style link. */
export function CardLink({ children, ...props }: LinkProps & { children: ReactNode }) {
  return (
    <Link {...props} className="inline-flex items-center gap-0.5 text-callout text-accent">
      {children}
      <ChevronRight aria-hidden className="size-3" strokeWidth={2.25} />
    </Link>
  );
}

/** One number that matters, what it means, and an optional chart under it. */
export function StatCard({
  icon: Icon,
  label,
  value,
  unit,
  detail,
  tone = "neutral",
  to,
  children,
}: {
  icon: LucideIcon;
  label: string;
  value: ReactNode;
  unit?: ReactNode;
  detail?: ReactNode;
  tone?: Tone;
  /** Where the card leads (Analytics, Routes…). */
  to?: LinkProps["to"];
  children?: ReactNode;
}) {
  const body = (
    <>
      <div className="flex items-center gap-2">
        <span
          className={cn(
            "flex size-6 shrink-0 items-center justify-center rounded-md",
            toneIcon[tone],
          )}
        >
          <Icon aria-hidden className="size-3.5" strokeWidth={2} />
        </span>
        <span className="truncate text-callout text-secondary">{label}</span>
      </div>
      <div className="mt-2 flex items-baseline gap-1">
        <span className={cn("truncate text-title1 tabular", toneText[tone])}>{value}</span>
        {unit ? <span className="text-callout text-secondary">{unit}</span> : null}
      </div>
      {detail ? <div className="truncate text-callout text-secondary tabular">{detail}</div> : null}
      {children ? <div className="mt-auto pt-2">{children}</div> : null}
    </>
  );
  const className =
    "flex min-h-[132px] min-w-0 flex-col rounded-card border-hairline border-inset bg-surface-inset px-4 py-3.5 outline-offset-0";
  return to ? (
    <Link to={to} className={cn(className, "transition-colors hover:bg-surface-hover")}>
      {body}
    </Link>
  ) : (
    <div className={className}>{body}</div>
  );
}

/** Stat cards in a row that wraps: 2 across in a narrow pane, up to 6 on a wide one. */
export function StatGrid({ children }: { children: ReactNode }) {
  return <div className="grid grid-cols-2 gap-3 @3xl:grid-cols-3 @7xl:grid-cols-6">{children}</div>;
}

export interface Segment {
  label: string;
  value: number;
  tone: Tone;
}

const toneBar: Record<Tone, string> = {
  neutral: "bg-idle",
  healthy: "bg-healthy",
  warning: "bg-warning",
  error: "bg-error",
  accent: "bg-accent",
};

/** Parts of a whole as one bar, with a legend of each part's share. */
export function DistributionBar({
  label,
  segments,
  format,
}: {
  label: string;
  segments: readonly Segment[];
  format: (share: number) => string;
}) {
  const total = segments.reduce((sum, s) => sum + s.value, 0);
  const shown = segments.filter((s) => s.value > 0);
  return (
    <div className="flex flex-col gap-2.5">
      <div
        role="img"
        aria-label={`${label}: ${shown.map((s) => `${s.label} ${format(s.value / total)}`).join(", ")}`}
        className="flex h-2 w-full gap-0.5 overflow-hidden rounded-full bg-surface-control"
      >
        {total > 0
          ? shown.map((s) => (
              <div
                key={s.label}
                className={cn("h-full first:rounded-l-full last:rounded-r-full", toneBar[s.tone])}
                style={{ width: `${(s.value / total) * 100}%` }}
              />
            ))
          : null}
      </div>
      <ul className="grid grid-cols-2 gap-x-4 gap-y-1">
        {segments.map((s) => (
          <li key={s.label} className="flex items-center gap-2 text-callout">
            <span aria-hidden className={cn("size-2 shrink-0 rounded-full", toneBar[s.tone])} />
            <span className="min-w-0 flex-1 truncate text-secondary">{s.label}</span>
            <span className="tabular">{total > 0 ? format(s.value / total) : "–"}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Label–value rows inside a card. */
export function MetricList({ items }: { items: readonly { label: string; value: ReactNode }[] }) {
  return (
    <dl className="flex flex-col divide-y-(length:--hairline) divide-inset">
      {items.map((item) => (
        <div key={item.label} className="flex items-center justify-between gap-3 py-1.5">
          <dt className="truncate text-callout text-secondary">{item.label}</dt>
          <dd className="truncate text-callout tabular">{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** The dashboard's shape while it loads. */
export function DashboardSkeleton({ label }: { label: string }) {
  return (
    <div role="status" aria-busy aria-label={label} className="@container flex-1 overflow-hidden">
      <div className="mx-auto flex w-full max-w-[1680px] flex-col gap-4 px-6 pt-2">
        <Skeleton className="h-16 rounded-card" />
        <StatGrid>
          {Array.from({ length: 6 }, (_, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: placeholders have no identity
            <Skeleton key={i} className="h-[132px] rounded-card" />
          ))}
        </StatGrid>
        <div className="grid gap-3 @5xl:grid-cols-3">
          <Skeleton className="h-72 rounded-card @5xl:col-span-2" />
          <Skeleton className="h-72 rounded-card" />
        </div>
        <div className="grid gap-3 @5xl:grid-cols-2">
          <Skeleton className="h-64 rounded-card" />
          <Skeleton className="h-64 rounded-card" />
        </div>
      </div>
    </div>
  );
}

export { toneIcon };
