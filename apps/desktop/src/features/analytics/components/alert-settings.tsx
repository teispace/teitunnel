import { GroupedRow, GroupedSection, SkeletonSection } from "@/components/patterns/grouped-list";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import type { AlertRules } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useAlertRules, useSetAlertRules } from "../queries";

const DOWN_AFTER = [1, 2, 3, 5, 10] as const;
const PERCENTS = [1, 2, 5, 10, 25] as const;
const LATENCIES = [500, 1_000, 2_000, 3_000, 5_000, 10_000] as const;

/** Options for a numeric menu, keeping a value set elsewhere (e.g. the CLI) listed. */
function options(values: readonly number[], current: number, label: (n: number) => string) {
  const all = [...new Set([...values, current])].sort((a, b) => a - b);
  return all.map((n) => ({ value: String(n), label: label(n) }));
}

/** Settings ▸ General ▸ Alerts: what raises an alert (recorded in Activity, notified). */
export function AlertSettings() {
  const { data: rules } = useAlertRules();
  const save = useSetAlertRules();
  if (!rules) return <SkeletonSection rows={5} />;
  const update = (patch: Partial<AlertRules>) => save.mutate({ ...rules, ...patch });
  return (
    <GroupedSection title={t("alerts.title")} footer={t("alerts.footer")}>
      <GroupedRow label={t("alerts.routeDown")} description={t("alerts.routeDownDetail")}>
        <Select
          label={t("alerts.downAfterLabel")}
          disabled={!rules.routeDown}
          options={options(DOWN_AFTER, rules.downAfter, (count) =>
            t("alerts.downAfter", { count }),
          )}
          value={String(rules.downAfter)}
          onValueChange={(v) => update({ downAfter: Number(v) })}
        />
        <Switch
          aria-label={t("alerts.routeDown")}
          checked={rules.routeDown}
          onCheckedChange={(routeDown) => update({ routeDown })}
        />
      </GroupedRow>
      <GroupedRow label={t("alerts.recovered")}>
        <Switch
          aria-label={t("alerts.recovered")}
          checked={rules.recovered}
          onCheckedChange={(recovered) => update({ recovered })}
        />
      </GroupedRow>
      <GroupedRow label={t("alerts.errorRate")} description={t("alerts.errorRateDetail")}>
        <Select
          label={t("alerts.percentLabel")}
          disabled={!rules.errorRate}
          options={options(PERCENTS, rules.errorRatePercent, (value) =>
            t("alerts.percent", { value }),
          )}
          value={String(rules.errorRatePercent)}
          onValueChange={(v) => update({ errorRatePercent: Number(v) })}
        />
        <Switch
          aria-label={t("alerts.errorRate")}
          checked={rules.errorRate}
          onCheckedChange={(errorRate) => update({ errorRate })}
        />
      </GroupedRow>
      <GroupedRow label={t("alerts.latency")} description={t("alerts.latencyDetail")}>
        <Select
          label={t("alerts.latencyLabel")}
          disabled={!rules.latency}
          options={options(LATENCIES, rules.latencyMs, (value) => t("alerts.ms", { value }))}
          value={String(rules.latencyMs)}
          onValueChange={(v) => update({ latencyMs: Number(v) })}
        />
        <Switch
          aria-label={t("alerts.latency")}
          checked={rules.latency}
          onCheckedChange={(latency) => update({ latency })}
        />
      </GroupedRow>
      <GroupedRow label={t("alerts.connectorDown")}>
        <Switch
          aria-label={t("alerts.connectorDown")}
          checked={rules.connectorDown}
          onCheckedChange={(connectorDown) => update({ connectorDown })}
        />
      </GroupedRow>
      {save.error ? (
        <p role="alert" className="py-2 text-callout text-error">
          {toIpcError(save.error).message}
        </p>
      ) : null}
    </GroupedSection>
  );
}
