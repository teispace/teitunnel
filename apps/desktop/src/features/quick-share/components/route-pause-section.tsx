import { toast } from "sonner";
import { InspectorSection } from "@/components/patterns/inspector";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useSchedules, useSetSharePaused } from "../queries";
import { scheduleLabel } from "../schedule";
import { PauseButton } from "./pause-button";
import { ScheduleButton } from "./schedule-editor";

/**
 * Pause and schedule for one of this computer's routes: visitors get the paused page
 * (the address stays) while it's paused or outside its hours.
 */
export function RoutePauseSection({
  accountId,
  hostname,
  paused,
}: {
  accountId: string;
  hostname: string;
  paused: boolean;
}) {
  const pause = useSetSharePaused();
  const { data: schedules } = useSchedules();
  const schedule = schedules?.find((s) => s.accountId === accountId && s.hostname === hostname);
  const note = paused
    ? t("quickShare.pause.detail")
    : schedule
      ? t("quickShare.schedule.line", { schedule: scheduleLabel(schedule.schedule) })
      : t("routes.pause.hint");
  return (
    <InspectorSection title={t("routes.pause.title")}>
      <div className="flex items-center gap-2">
        <PauseButton
          paused={paused}
          pending={pause.isPending}
          onToggle={() =>
            pause.mutate(
              { accountId, hostname, paused: !paused },
              { onError: (error) => toast.error(toIpcError(error).message) },
            )
          }
        />
        <ScheduleButton
          accountId={accountId}
          hostname={hostname}
          current={schedule?.schedule ?? null}
        />
        <p className="min-w-0 text-callout text-secondary">{note}</p>
      </div>
    </InspectorSection>
  );
}
