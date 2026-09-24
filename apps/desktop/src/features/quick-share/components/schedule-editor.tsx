import { CalendarClock } from "lucide-react";
import { useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { Schedule, Weekday } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useSetSchedule } from "../queries";
import { systemTimeZone, WEEKDAYS, weekdayLabel } from "../schedule";

const OFFICE: Schedule = {
  days: ["mon", "tue", "wed", "thu", "fri"],
  from: "09:00",
  to: "18:00",
  timeZone: null,
};

/** The schedule button of a share on your domain, with its editor in a popover. */
export function ScheduleButton({
  accountId,
  hostname,
  current,
}: {
  accountId: string;
  hostname: string;
  current: Schedule | null;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tooltip content={t("quickShare.schedule.button")}>
        <PopoverTrigger asChild>
          <IconButton
            icon={CalendarClock}
            label={t("quickShare.schedule.button")}
            variant="secondary"
            size="lg"
          />
        </PopoverTrigger>
      </Tooltip>
      <PopoverContent align="end" className="w-72">
        {open ? (
          <ScheduleForm
            accountId={accountId}
            hostname={hostname}
            current={current}
            onDone={() => setOpen(false)}
          />
        ) : null}
      </PopoverContent>
    </Popover>
  );
}

function ScheduleForm({
  accountId,
  hostname,
  current,
  onDone,
}: {
  accountId: string;
  hostname: string;
  current: Schedule | null;
  onDone: () => void;
}) {
  const [draft, setDraft] = useState<Schedule>(current ?? OFFICE);
  const save = useSetSchedule();
  const remove = useSetSchedule();
  const fromId = useId();
  const toId = useId();
  const error = save.error ?? remove.error;
  const toggle = (day: Weekday) =>
    setDraft((d) => ({
      ...d,
      days: d.days.includes(day)
        ? d.days.filter((x) => x !== day)
        : WEEKDAYS.filter((x) => x === day || d.days.includes(x)),
    }));

  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        save.mutate({ accountId, hostname, schedule: draft }, { onSuccess: onDone });
      }}
    >
      <div>
        <h3 className="text-headline">{t("quickShare.schedule.title")}</h3>
        <p className="text-footnote text-secondary">{t("quickShare.schedule.detail")}</p>
      </div>
      <fieldset className="flex gap-1">
        <legend className="sr-only">{t("quickShare.schedule.days")}</legend>
        {WEEKDAYS.map((day) => {
          const on = draft.days.includes(day);
          return (
            <button
              key={day}
              type="button"
              aria-pressed={on}
              aria-label={weekdayLabel(day, "long")}
              onClick={() => toggle(day)}
              className={cn(
                "h-6 flex-1 rounded-control text-footnote font-medium outline-offset-1 transition-colors transition-snappy",
                on ? "bg-accent-fill text-on-accent" : "bg-surface-control text-secondary",
              )}
            >
              {weekdayLabel(day).slice(0, 2)}
            </button>
          );
        })}
      </fieldset>
      <div className="flex items-center gap-2 text-callout">
        <label htmlFor={fromId} className="text-secondary">
          {t("quickShare.schedule.from")}
        </label>
        <Input
          id={fromId}
          type="time"
          required
          value={draft.from}
          onChange={(e) => setDraft((d) => ({ ...d, from: e.target.value }))}
          className="h-7 flex-1 tabular"
        />
        <label htmlFor={toId} className="text-secondary">
          {t("quickShare.schedule.to")}
        </label>
        <Input
          id={toId}
          type="time"
          required
          value={draft.to}
          onChange={(e) => setDraft((d) => ({ ...d, to: e.target.value }))}
          className="h-7 flex-1 tabular"
        />
      </div>
      <p className="text-footnote text-secondary">
        {t("quickShare.schedule.timeZone", { zone: draft.timeZone ?? systemTimeZone() })}
      </p>
      {error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(error).message}
        </p>
      ) : null}
      <div className="flex items-center gap-2">
        {current ? (
          <Button
            type="button"
            variant="destructive"
            size="sm"
            pending={remove.isPending}
            disabled={save.isPending}
            onClick={() =>
              remove.mutate({ accountId, hostname, schedule: null }, { onSuccess: onDone })
            }
          >
            {t("quickShare.schedule.remove")}
          </Button>
        ) : null}
        <Button
          type="submit"
          variant="primary"
          size="sm"
          className="ml-auto"
          pending={save.isPending}
          disabled={draft.days.length === 0 || remove.isPending}
        >
          {t("quickShare.schedule.save")}
        </Button>
      </div>
    </form>
  );
}
