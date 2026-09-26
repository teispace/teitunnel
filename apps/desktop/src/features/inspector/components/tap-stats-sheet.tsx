import { useState } from "react";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Sheet, SheetClose, SheetContent } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { TrafficStats } from "@/features/analytics";
import { rangeOptions } from "@/features/analytics/model";
import { t } from "@/lib/i18n";
import type { AnalyticsRange } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useInspectorSettings, useTapStats } from "../queries";

const HOURS: Record<AnalyticsRange, number> = { hour: 1, day: 24, week: 168, month: 720 };

/** Ranges the inspector's history covers (it keeps `retentionHours`), at least the hour. */
export function rangesKept(retentionHours: number): AnalyticsRange[] {
  return (Object.keys(HOURS) as AnalyticsRange[]).filter(
    (range) => range === "hour" || HOURS[range] <= retentionHours,
  );
}

interface TapStatsSheetProps {
  /** The share's or route's tap; null closes the sheet. */
  tap: string | null;
  name: string;
  onClose: () => void;
}

/**
 * One share's or route's numbers from the inspector: exact, for any share (Quick Shares
 * included, which Cloudflare's analytics don't cover), over the history it keeps.
 */
export function TapStatsSheet({ tap, name, onClose }: TapStatsSheetProps) {
  const [range, setRange] = useState<AnalyticsRange>("hour");
  const settings = useInspectorSettings();
  const kept = rangesKept(settings.data?.retentionHours ?? 24);
  const shown = kept.includes(range) ? range : "hour";
  const stats = useTapStats(tap, shown);
  return (
    <Sheet open={tap !== null} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        title={t("inspector.stats.title")}
        description={name}
        width="lg"
        footer={
          <SheetClose asChild>
            <Button variant="primary">{t("common.done")}</Button>
          </SheetClose>
        }
      >
        <div className="flex flex-col gap-4">
          {kept.length > 1 ? (
            <SegmentedControl
              label={t("analytics.rangeLabel")}
              size="sm"
              segments={rangeOptions().filter((option) => kept.includes(option.value))}
              value={shown}
              onValueChange={setRange}
            />
          ) : null}
          {stats.error ? (
            <p role="alert" className="text-callout text-error">
              {toIpcError(stats.error).message}
            </p>
          ) : stats.data ? (
            <TrafficStats stats={stats.data} range={shown} />
          ) : stats.data === null ? (
            <p className="text-callout text-secondary">{t("inspector.stats.gone")}</p>
          ) : (
            <div role="status" aria-busy aria-label={t("inspector.stats.title")}>
              <Skeleton className="h-28" />
            </div>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
