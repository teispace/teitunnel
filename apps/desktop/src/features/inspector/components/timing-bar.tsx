import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { Timings } from "@/lib/ipc/bindings";
import { formatMs } from "../model";

interface Segment {
  key: "connect" | "wait" | "receive";
  ms: number;
  className: string;
}

/** The phases of a request from its timings (microseconds since it arrived). */
export function phases(timings: Timings, durationMs: number | null): Segment[] | null {
  const connected = timings.upstreamConnectedUs;
  const firstByte = timings.firstByteUs;
  const end = timings.completeUs ?? (durationMs === null ? null : durationMs * 1000);
  if (end === null || end <= 0) return null;
  const segments: Segment[] = [];
  if (connected !== null) {
    segments.push({ key: "connect", ms: connected / 1000, className: "bg-idle" });
  }
  if (firstByte !== null) {
    segments.push({
      key: "wait",
      ms: Math.max(0, firstByte - (connected ?? 0)) / 1000,
      className: "bg-accent",
    });
    segments.push({
      key: "receive",
      ms: Math.max(0, end - firstByte) / 1000,
      className: "bg-healthy",
    });
  }
  return segments.length > 0 ? segments : null;
}

/** Connect, waiting for the first byte, and receiving, as one bar with a legend. */
export function TimingBar({
  timings,
  durationMs,
}: {
  timings: Timings;
  durationMs: number | null;
}) {
  const segments = phases(timings, durationMs);
  if (!segments) {
    return <p className="text-callout text-secondary">{t("inspector.timing.unavailable")}</p>;
  }
  const total = segments.reduce((sum, s) => sum + s.ms, 0) || 1;
  return (
    <div className="flex flex-col gap-1.5">
      <div
        role="img"
        aria-label={segments
          .map((s) => `${t(`inspector.timing.${s.key}`)} ${formatMs(s.ms)}`)
          .join(", ")}
        className="flex h-2 w-full overflow-hidden rounded-full bg-surface-inset"
      >
        {segments.map((s) => (
          <span
            key={s.key}
            className={cn("h-full min-w-px", s.className)}
            style={{ width: `${(s.ms / total) * 100}%` }}
          />
        ))}
      </div>
      <ul className="flex flex-wrap gap-x-3 gap-y-0.5 text-callout text-secondary">
        {segments.map((s) => (
          <li key={s.key} className="flex items-center gap-1">
            <span aria-hidden className={cn("size-2 rounded-full", s.className)} />
            {t(`inspector.timing.${s.key}`)}
            <span className="text-primary tabular">{formatMs(s.ms)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
