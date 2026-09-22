/** "12 s", "4 min", "1 h 5 min" (compact, for elapsed time). */
export function formatDuration(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

const counter = new Intl.NumberFormat();

/** "1 request", "1,234 requests". */
export function formatCount(count: number, noun: string, plural = `${noun}s`): string {
  return `${counter.format(count)} ${count === 1 ? noun : plural}`;
}

/** Strips the scheme for display: "https://a.b" → "a.b", "http://localhost:3000" → "localhost:3000". */
export function stripScheme(url: string): string {
  return url.replace(/^[a-z]+:\/\//i, "");
}
