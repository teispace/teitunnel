import { formatBytes } from "@/features/snapshots/format";
import { currentLanguage, type MessageKey, t } from "@/lib/i18n";
import type {
  ExchangeRow,
  HeaderView,
  ReplayInput,
  TrafficFormat,
  WebhookSender,
} from "@/lib/ipc/bindings";

/** The status filter: every class, one class, or failed exchanges. */
export type StatusFilter = "all" | "2" | "3" | "4" | "5" | "errors";

/** What the list shows. `search` goes to the inspector (it searches bodies too). */
export interface Filters {
  status: StatusFilter;
  /** A method, or `""` for any. */
  method: string;
  /** Text in the path or host, case-insensitive. */
  text: string;
  /** At least this slow, in milliseconds (`0`: any). */
  minDurationMs: number;
}

export const noFilters: Filters = { status: "all", method: "", text: "", minDurationMs: 0 };

export const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;
export const DURATIONS = [0, 100, 500, 1000, 5000] as const;

/** Whether a row passes the list's filters (the same rules as the inspector's query). */
export function matches(row: ExchangeRow, filters: Filters): boolean {
  if (filters.method && row.method.toUpperCase() !== filters.method) return false;
  if (filters.status === "errors") {
    if (row.state !== "failed") return false;
  } else if (filters.status !== "all") {
    if (row.status === null || Math.floor(row.status / 100) !== Number(filters.status)) {
      return false;
    }
  }
  if (filters.minDurationMs > 0 && (row.durationMs ?? 0) < filters.minDurationMs) return false;
  const text = filters.text.trim().toLowerCase();
  if (text && !row.path.toLowerCase().includes(text) && !row.host.toLowerCase().includes(text)) {
    return false;
  }
  return true;
}

export const isFiltered = (filters: Filters) =>
  filters.status !== "all" ||
  filters.method !== "" ||
  filters.text.trim() !== "" ||
  filters.minDurationMs > 0;

/** "84 ms", "1.24 s", "12.5 s". */
export function formatMs(ms: number): string {
  const format = (value: number, unit: "millisecond" | "second", digits: number) =>
    new Intl.NumberFormat(currentLanguage(), {
      style: "unit",
      unit,
      unitDisplay: "short",
      maximumFractionDigits: digits,
    }).format(value);
  if (ms < 1000) return format(Math.round(ms), "millisecond", 0);
  return format(ms / 1000, "second", ms < 10_000 ? 2 : 1);
}

/** A row's size: the response body, else the request body. */
export function rowSize(row: ExchangeRow): string {
  const bytes = row.responseBytes || row.requestBytes;
  return bytes ? formatBytes(bytes) : "";
}

const clock = new Map<string, Intl.DateTimeFormat>();
/** "14:03:27" in the user's locale. */
export function formatClock(at: number | null): string {
  if (at === null) return "";
  const key = currentLanguage();
  let format = clock.get(key);
  if (!format) {
    format = new Intl.DateTimeFormat(key, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hourCycle: "h23",
    });
    clock.set(key, format);
  }
  return format.format(at);
}

/** The status cell: the code, or where the exchange is. */
export function statusText(row: ExchangeRow): string {
  if (row.status !== null) return String(row.status);
  if (row.state === "failed") return t("inspector.list.failed");
  return t("inspector.list.pending");
}

export type Tone = "healthy" | "warning" | "error" | "neutral";

export function statusTone(status: number | null, failed = false): Tone {
  if (failed) return "error";
  if (status === null) return "neutral";
  if (status >= 500) return "error";
  if (status >= 400) return "warning";
  if (status >= 200 && status < 300) return "healthy";
  return "neutral";
}

export const toneText: Record<Tone, string> = {
  healthy: "text-healthy",
  warning: "text-warning",
  error: "text-error",
  neutral: "text-secondary",
};

export const providerNames: Record<WebhookSender, string> = {
  stripe: "Stripe",
  gitHub: "GitHub",
  slack: "Slack",
  shopify: "Shopify",
  standardWebhooks: "Standard Webhooks",
  twilio: "Twilio",
  linear: "Linear",
  discord: "Discord",
};

export const FORMATS: readonly TrafficFormat[] = [
  "curl",
  "httpie",
  "fetch",
  "raw",
  "har",
  "json",
  "markdown",
];

export const formatLabel = (format: TrafficFormat) =>
  t(`inspector.export.formats.${format}` as MessageKey);

// Bodies ----------------------------------------------------------------------------

/** Indented JSON, or `null` when the text isn't JSON. */
export function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return null;
  }
}

/** `a=1&b=two` as pairs, in order (repeated names kept). */
export function parseForm(text: string): [string, string][] {
  return [...new URLSearchParams(text.trim()).entries()];
}

export interface Part {
  headers: [string, string][];
  /** The part's name from `Content-Disposition`. */
  name: string | null;
  /** The uploaded file's name, when it's a file. */
  filename: string | null;
  body: string;
}

/** The parts of a `multipart/*` body, split on the boundary from its content type. */
export function parseMultipart(text: string, contentType: string | null): Part[] {
  const boundary = /boundary="?([^";]+)"?/i.exec(contentType ?? "")?.[1];
  if (!boundary) return [];
  const parts: Part[] = [];
  for (const chunk of text.split(`--${boundary}`).slice(1)) {
    if (chunk.startsWith("--")) break;
    const content = chunk.replace(/^\r?\n/, "").replace(/\r?\n$/, "");
    const split = /\r?\n\r?\n/.exec(content);
    const head = split ? content.slice(0, split.index) : content;
    const body = split ? content.slice(split.index + split[0].length) : "";
    const headers = head
      .split(/\r?\n/)
      .filter(Boolean)
      .map((line): [string, string] => {
        const colon = line.indexOf(":");
        return colon < 0
          ? [line.trim(), ""]
          : [line.slice(0, colon).trim(), line.slice(colon + 1).trim()];
      });
    const disposition =
      headers.find(([name]) => name.toLowerCase() === "content-disposition")?.[1] ?? "";
    parts.push({
      headers,
      name: /\bname="([^"]*)"/i.exec(disposition)?.[1] ?? null,
      filename: /\bfilename="([^"]*)"/i.exec(disposition)?.[1] ?? null,
      body,
    });
  }
  return parts;
}

/** Bytes from base64. */
export function fromBase64(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** Bytes shown as hex at most (the rest is summarised). */
export const HEX_LIMIT = 16 * 1024;

/** `00000000  48 65 6c 6c 6f  |Hello|` lines, 16 bytes each. */
export function hexDump(bytes: Uint8Array, limit = HEX_LIMIT): string {
  const lines: string[] = [];
  const end = Math.min(bytes.length, limit);
  for (let offset = 0; offset < end; offset += 16) {
    const row = bytes.subarray(offset, Math.min(offset + 16, end));
    const hex = [...row].map((b) => b.toString(16).padStart(2, "0"));
    const ascii = [...row].map((b) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : "."));
    lines.push(
      `${offset.toString(16).padStart(8, "0")}  ${hex.slice(0, 8).join(" ").padEnd(23)}  ${hex
        .slice(8)
        .join(" ")
        .padEnd(23)}  |${ascii.join("")}|`,
    );
  }
  return lines.join("\n");
}

/** Text as UTF-8 bytes (for a hex view of a text body). */
export const utf8 = (text: string) => new TextEncoder().encode(text);

// Replay ----------------------------------------------------------------------------

/** Headers as `Name: value` lines. */
export const headersText = (headers: readonly HeaderView[]) =>
  headers.map((h) => `${h.name}: ${h.value}`).join("\n");

/** `Name: value` lines as pairs (blank lines and lines without a name are skipped). */
export function parseHeaders(text: string): [string, string][] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .flatMap((line): [string, string][] => {
      const colon = line.indexOf(":");
      if (colon <= 0) return [];
      return [[line.slice(0, colon).trim(), line.slice(colon + 1).trim()]];
    });
}

/** Headers Teitunnel sets itself; editing them makes no sense in a replay. */
const HOP = new Set(["host", "content-length", "connection", "transfer-encoding"]);

export interface ReplayDraft {
  method: string;
  path: string;
  headers: string;
  body: string;
  times: number;
  resign: boolean;
}

export interface ReplayOriginal {
  method: string;
  /** Path and query. */
  path: string;
  headers: readonly HeaderView[];
  body: string | null;
}

/**
 * What changed between the captured request and the edited one. Only changes are sent:
 * a header left alone keeps its original value (even a masked one), so masking never
 * leaks into a replay.
 */
export function replayInput(original: ReplayOriginal, draft: ReplayDraft): ReplayInput {
  const input: ReplayInput = {};
  const method = draft.method.trim().toUpperCase();
  if (method && method !== original.method.toUpperCase()) input.method = method;
  const path = draft.path.trim();
  if (path && path !== original.path) input.path = path;
  const before = new Map<string, string[]>();
  for (const h of original.headers) {
    const name = h.name.toLowerCase();
    before.set(name, [...(before.get(name) ?? []), h.value]);
  }
  const after = new Map<string, string[]>();
  const names = new Map<string, string>();
  for (const [name, value] of parseHeaders(draft.headers)) {
    const key = name.toLowerCase();
    after.set(key, [...(after.get(key) ?? []), value]);
    if (!names.has(key)) names.set(key, name);
  }
  const set: [string, string][] = [];
  for (const [key, values] of after) {
    if (HOP.has(key)) continue;
    if ((before.get(key) ?? []).join("\n") === values.join("\n")) continue;
    for (const value of values) set.push([names.get(key) ?? key, value]);
  }
  const remove = [...before.keys()].filter((key) => !after.has(key) && !HOP.has(key));
  if (set.length > 0) input.setHeaders = set;
  if (remove.length > 0) input.removeHeaders = remove;
  if (draft.body !== (original.body ?? "")) input.body = draft.body;
  const times = Math.min(100, Math.max(1, Math.round(draft.times) || 1));
  if (times > 1) input.times = times;
  if (draft.resign) input.resign = true;
  return input;
}

// Compare ---------------------------------------------------------------------------

export type DiffLine = { kind: "same" | "added" | "removed"; text: string };

/** Lines compared at most on each side (the table is quadratic). */
export const DIFF_LIMIT = 2000;

/** A line diff of `a` → `b` (longest common subsequence), or `null` when too large. */
export function diffLines(a: string, b: string): DiffLine[] | null {
  const left = a === "" ? [] : a.split("\n");
  const right = b === "" ? [] : b.split("\n");
  if (left.length > DIFF_LIMIT || right.length > DIFF_LIMIT) return null;
  const n = left.length;
  const m = right.length;
  // lengths[i][j]: LCS of left[i..] and right[j..], in one flat array.
  const lengths = new Uint16Array((n + 1) * (m + 1));
  const at = (i: number, j: number) => i * (m + 1) + j;
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lengths[at(i, j)] =
        left[i] === right[j]
          ? (lengths[at(i + 1, j + 1)] ?? 0) + 1
          : Math.max(lengths[at(i + 1, j)] ?? 0, lengths[at(i, j + 1)] ?? 0);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (left[i] === right[j]) {
      out.push({ kind: "same", text: left[i] ?? "" });
      i++;
      j++;
    } else if ((lengths[at(i + 1, j)] ?? 0) >= (lengths[at(i, j + 1)] ?? 0)) {
      out.push({ kind: "removed", text: left[i] ?? "" });
      i++;
    } else {
      out.push({ kind: "added", text: right[j] ?? "" });
      j++;
    }
  }
  for (; i < n; i++) out.push({ kind: "removed", text: left[i] ?? "" });
  for (; j < m; j++) out.push({ kind: "added", text: right[j] ?? "" });
  return out;
}

export interface HeaderDiff {
  name: string;
  left: string | null;
  right: string | null;
}

/** Headers of two exchanges side by side (names lowercased, repeated values joined). */
export function diffHeaders(a: readonly HeaderView[], b: readonly HeaderView[]): HeaderDiff[] {
  const collect = (headers: readonly HeaderView[]) => {
    const map = new Map<string, string>();
    for (const h of headers) {
      const name = h.name.toLowerCase();
      const had = map.get(name);
      map.set(name, had === undefined ? h.value : `${had}, ${h.value}`);
    }
    return map;
  };
  const left = collect(a);
  const right = collect(b);
  const names = [...new Set([...left.keys(), ...right.keys()])].sort();
  return names.map((name) => ({
    name,
    left: left.get(name) ?? null,
    right: right.get(name) ?? null,
  }));
}

/** Body text for comparing: pretty JSON where it parses. */
export const comparable = (text: string | null) => (text ? (prettyJson(text) ?? text) : "");
