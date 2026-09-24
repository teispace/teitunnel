import { useState } from "react";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { UptimeBar } from "@/lib/ipc/bindings";
import { formatPercent } from "../model";

const time = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

/** How a slice looks: no data is a short grey tick (not colour alone, DESIGN §1). */
function tone(bar: UptimeBar): string {
  if (bar.checks === 0) return "h-1.5 bg-surface-control";
  const share = bar.up / bar.checks;
  if (share === 1) return "h-full bg-healthy";
  return share >= 0.5 ? "h-full bg-warning" : "h-full bg-error";
}

function describe(bar: UptimeBar): string {
  const from = time.format(bar.start);
  return bar.checks === 0
    ? t("uptime.barEmpty", { from })
    : t("uptime.bar", { from, percent: formatPercent(bar.up / bar.checks) });
}

/**
 * A status-page strip: one slice per stretch of the range, green when every check
 * passed. Hovering a slice says when it was and how much of it was up.
 */
export function UptimeStrip({ bars, label }: { bars: readonly UptimeBar[]; label: string }) {
  const [hovered, setHovered] = useState<number | null>(null);
  const shown = hovered === null ? null : bars[hovered];
  return (
    <div className="flex flex-col gap-1">
      <div
        role="img"
        aria-label={label}
        className="flex h-6 items-end gap-px"
        onMouseLeave={() => setHovered(null)}
      >
        {bars.map((bar, index) => (
          // biome-ignore lint/a11y/noStaticElementInteractions: hover only fills the readout; the strip's label and the uptime above say the same
          <div
            key={bar.start}
            className="flex h-full min-w-0 flex-1 items-end"
            onMouseEnter={() => setHovered(index)}
          >
            <div className={cn("w-full rounded-[1px]", tone(bar))} />
          </div>
        ))}
      </div>
      <p aria-hidden className="min-h-4 text-footnote text-secondary tabular">
        {shown ? describe(shown) : ""}
      </p>
    </div>
  );
}
