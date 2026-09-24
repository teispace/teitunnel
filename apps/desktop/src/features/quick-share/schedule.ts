import { currentLanguage, t } from "@/lib/i18n";
import type { RouteSchedule, Schedule, Weekday } from "@/lib/ipc/bindings";

/** Monday first, as schedules store them. */
export const WEEKDAYS: readonly Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** 2024-01-01 was a Monday: the day names come from the system, in its language. */
function dayName(day: Weekday, width: "short" | "long" = "short"): string {
  const date = new Date(Date.UTC(2024, 0, 1 + WEEKDAYS.indexOf(day), 12));
  return new Intl.DateTimeFormat(currentLanguage(), { weekday: width, timeZone: "UTC" }).format(
    date,
  );
}

export function weekdayLabel(day: Weekday, width: "short" | "long" = "short"): string {
  return dayName(day, width);
}

/** "09:00" in the system's style ("9:00 AM" in English). */
export function clock(hhmm: string): string {
  const [hours, minutes] = hhmm.split(":").map(Number);
  const date = new Date(Date.UTC(2024, 0, 1, hours ?? 0, minutes ?? 0));
  return new Intl.DateTimeFormat(currentLanguage(), {
    hour: "numeric",
    minute: "2-digit",
    timeZone: "UTC",
  }).format(date);
}

/** "Mon–Fri", "Sat, Sun", "Every day". */
export function daysLabel(days: readonly Weekday[]): string {
  if (days.length === 7) return t("quickShare.schedule.everyDay");
  const indexes = days.map((d) => WEEKDAYS.indexOf(d)).sort((a, b) => a - b);
  const first = indexes[0];
  const last = indexes[indexes.length - 1];
  const consecutive =
    first !== undefined && last !== undefined && last - first === indexes.length - 1;
  if (consecutive && indexes.length >= 3) {
    return t("quickShare.schedule.range", {
      from: dayName(WEEKDAYS[first] ?? "mon"),
      to: dayName(WEEKDAYS[last] ?? "sun"),
    });
  }
  return new Intl.ListFormat(currentLanguage(), { style: "narrow", type: "unit" }).format(
    indexes.map((i) => dayName(WEEKDAYS[i] ?? "mon")),
  );
}

/** "Mon–Fri, 9:00 AM–6:00 PM" (with the time zone when one is named). */
export function scheduleLabel(schedule: Schedule): string {
  const hours =
    schedule.from === schedule.to
      ? t("quickShare.schedule.allDay")
      : t("quickShare.schedule.hours", { from: clock(schedule.from), to: clock(schedule.to) });
  const text = t("quickShare.schedule.summary", { days: daysLabel(schedule.days), hours });
  return schedule.timeZone ? `${text} (${schedule.timeZone})` : text;
}

/** "Paused until Mon 9:00 AM" / "On until 6:00 PM", or nothing without a next change. */
export function nextChangeLabel(entry: RouteSchedule): string | null {
  if (entry.nextChange === null) return null;
  const at = new Date(entry.nextChange);
  const when = new Intl.DateTimeFormat(currentLanguage(), {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  }).format(at);
  return entry.on
    ? t("quickShare.schedule.onUntil", { when })
    : t("quickShare.schedule.offUntil", { when });
}

/** The system's time zone, e.g. `Europe/Berlin`. */
export function systemTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}
