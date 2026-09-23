import { currentLanguage, t } from "@/lib/i18n";

const units = new Map<string, Intl.NumberFormat>();
function unit(value: number, name: "second" | "minute" | "hour") {
  const key = `${currentLanguage()}:${name}`;
  let format = units.get(key);
  if (!format) {
    format = new Intl.NumberFormat(currentLanguage(), {
      style: "unit",
      unit: name,
      unitDisplay: "short",
    });
    units.set(key, format);
  }
  return format.format(value);
}

/** "12 sec", "4 min", "1 hr 5 min" in English (compact, for elapsed time). */
export function formatDuration(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return unit(seconds, "second");
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return unit(minutes, "minute");
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? unit(hours, "hour") : `${unit(hours, "hour")} ${unit(rest, "minute")}`;
}

/** Strips the scheme for display: "https://a.b" → "a.b", "http://localhost:3000" → "localhost:3000". */
export function stripScheme(url: string): string {
  return url.replace(/^[a-z]+:\/\//i, "");
}

/** "Just now", "5 min ago", then a date and time (for timestamps in Unix ms). */
export function relativeTime(at: number | null): string {
  if (at === null) return "";
  const minutes = Math.round((Date.now() - at) / 60_000);
  if (minutes < 1) return t("time.justNow");
  if (minutes < 60) return t("time.minutesAgo", { count: minutes });
  return new Date(at).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
