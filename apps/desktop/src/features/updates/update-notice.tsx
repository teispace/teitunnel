import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import { useRestartToUpdate, useUpdateStatus } from "./queries";

/** A quiet sidebar footer once an update is downloaded: the version and a restart button. */
export function UpdateNotice() {
  const { data: status } = useUpdateStatus();
  const restart = useRestartToUpdate();
  if (status?.state.state !== "ready") return null;
  return (
    <section
      aria-label={t("updates.title")}
      className="flex flex-col gap-1.5 rounded-row bg-surface-pressed px-2.5 py-2"
    >
      <p className="text-callout">{t("updates.ready", { version: status.state.version })}</p>
      <Button size="sm" pending={restart.isPending} onClick={() => restart.mutate()}>
        {t("updates.restart")}
      </Button>
    </section>
  );
}
