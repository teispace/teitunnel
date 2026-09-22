import { cn } from "@/lib/cn";

/** Determinate when `value` (0–1) is given, otherwise indeterminate. */
export function ProgressBar({
  value,
  label,
  className,
}: {
  value?: number;
  label: string;
  className?: string;
}) {
  const determinate = value !== undefined;
  const clamped = determinate ? Math.min(1, Math.max(0, value)) : 0;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      {...(determinate ? { "aria-valuenow": Math.round(clamped * 100) } : {})}
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-surface-control", className)}
    >
      {determinate ? (
        <div
          className="h-full origin-left rounded-full bg-accent transition-transform transition-smooth"
          style={{ transform: `scaleX(${clamped})` }}
        />
      ) : (
        <div className="h-full w-2/5 animate-indeterminate rounded-full bg-accent" />
      )}
    </div>
  );
}
