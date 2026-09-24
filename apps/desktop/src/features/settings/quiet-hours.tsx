import { GroupedRow } from "@/components/patterns/grouped-list";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import type { QuietHours } from "@/lib/ipc/bindings";

const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

/** Every half hour of a day, labelled in the user's clock format. */
const halfHours = () =>
  Array.from({ length: 48 }, (_, i) => {
    const minutes = i * 30;
    const at = new Date(2000, 0, 1, Math.floor(minutes / 60), minutes % 60);
    return { value: String(minutes), label: time.format(at) };
  });

/** Settings ▸ Notifications ▸ Quiet hours: alerts are recorded but don't notify. */
export function QuietHoursRow({
  value,
  onChange,
}: {
  value: QuietHours;
  onChange: (quietHours: QuietHours) => void;
}) {
  const options = halfHours();
  return (
    <GroupedRow
      label={t("settings.notifications.quiet")}
      description={t("settings.notifications.quietDetail")}
    >
      <Select
        label={t("settings.notifications.quietFrom")}
        disabled={!value.enabled}
        options={options}
        value={String(value.from - (value.from % 30))}
        onValueChange={(from) => onChange({ ...value, from: Number(from) })}
      />
      <Select
        label={t("settings.notifications.quietTo")}
        disabled={!value.enabled}
        options={options}
        value={String(value.to - (value.to % 30))}
        onValueChange={(to) => onChange({ ...value, to: Number(to) })}
      />
      <Switch
        aria-label={t("settings.notifications.quiet")}
        checked={value.enabled}
        onCheckedChange={(enabled) => onChange({ ...value, enabled })}
      />
    </GroupedRow>
  );
}
