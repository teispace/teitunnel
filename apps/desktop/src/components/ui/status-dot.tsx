import { cn } from "@/lib/cn";

export type Status = "healthy" | "connecting" | "warning" | "error" | "idle";

const labels: Record<Status, string> = {
  healthy: "Healthy",
  connecting: "Connecting",
  warning: "Warning",
  error: "Error",
  idle: "Stopped",
};

/**
 * Status is never colour alone (DESIGN §6): each state also has a shape. Healthy is a
 * filled dot, idle a ring, warning a triangle, error a dot with a cross; connecting
 * pulses slowly (static under Reduce motion).
 */
export function StatusDot({
  status,
  label = labels[status],
  className,
}: {
  status: Status;
  label?: string;
  className?: string;
}) {
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox="0 0 10 10"
      className={cn(
        "size-2.5 shrink-0 transition-colors transition-smooth",
        status === "healthy" && "text-healthy",
        status === "connecting" && "animate-pulse-slow text-warning",
        status === "warning" && "text-warning",
        status === "error" && "text-error",
        status === "idle" && "text-idle",
        className,
      )}
    >
      {status === "warning" ? (
        <path d="M5 0.8 9.6 9H0.4Z" fill="currentColor" />
      ) : status === "idle" ? (
        <circle cx="5" cy="5" r="3.6" fill="none" stroke="currentColor" strokeWidth="1.5" />
      ) : (
        <circle cx="5" cy="5" r="4.2" fill="currentColor" />
      )}
      {status === "error" ? (
        <path
          d="M3.4 3.4 6.6 6.6M6.6 3.4 3.4 6.6"
          stroke="var(--surface-content)"
          strokeWidth="1.2"
          strokeLinecap="round"
        />
      ) : null}
    </svg>
  );
}
