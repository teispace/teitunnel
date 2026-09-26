import { formatDuration } from "@/lib/format";
import { type MessageKey, t } from "@/lib/i18n";
import type {
  BotMode,
  EdgeProtection,
  HeaderRule,
  LimitAction,
  QuotaKind,
  RateLimitSpec,
} from "@/lib/ipc/bindings";

/** Every field filled in (the IPC type has them optional: the core defaults them). */
export interface Protection {
  bots: BotMode;
  aiCrawlers: boolean;
  rateLimit: RateLimitSpec | null;
  requestHeaders: HeaderRule[];
  responseHeaders: HeaderRule[];
  bypassCache: boolean;
}

export const noProtection: Protection = {
  bots: "off",
  aiCrawlers: false,
  rateLimit: null,
  requestHeaders: [],
  responseHeaders: [],
  bypassCache: false,
};

export function complete(protection: EdgeProtection | undefined): Protection {
  return {
    bots: protection?.bots ?? "off",
    aiCrawlers: protection?.aiCrawlers ?? false,
    rateLimit: protection?.rateLimit ?? null,
    requestHeaders: protection?.requestHeaders ?? [],
    responseHeaders: protection?.responseHeaders ?? [],
    bypassCache: protection?.bypassCache ?? false,
  };
}

export function isOff(protection: Protection): boolean {
  return (
    protection.bots === "off" &&
    !protection.aiCrawlers &&
    protection.rateLimit === null &&
    protection.requestHeaders.length === 0 &&
    protection.responseHeaders.length === 0 &&
    !protection.bypassCache
  );
}

/** Rate limit periods Cloudflare supports, in seconds. */
export const periods = [10, 60, 120, 300, 600, 3600] as const;

/** "10 sec", "1 min", "1 hr". */
export const periodLabel = (seconds: number) => formatDuration(seconds * 1000);

export const botLabels: Record<BotMode, MessageKey> = {
  off: "protection.bots.off",
  challenge: "protection.bots.challenge",
  block: "protection.bots.block",
};

export const actionLabels: Record<LimitAction, MessageKey> = {
  block: "protection.rateLimit.action.block",
  challenge: "protection.rateLimit.action.challenge",
};

export const quotaLabels: Record<QuotaKind, MessageKey> = {
  custom: "protection.quota.custom",
  rateLimit: "protection.quota.rateLimit",
  transform: "protection.quota.transform",
  cache: "protection.quota.cache",
};

/** "30 requests per 1 min per visitor, then blocked". */
export function describeRateLimit(limit: RateLimitSpec | null): string {
  if (!limit) return t("protection.rateLimit.none");
  const vars = { count: limit.requests, period: periodLabel(limit.period) };
  return limit.action === "block"
    ? t("protection.rateLimit.summaryBlock", vars)
    : t("protection.rateLimit.summaryChallenge", vars);
}

/** "2 request, 1 response". */
export function describeHeaders(protection: Protection): string {
  const request = protection.requestHeaders.length;
  const response = protection.responseHeaders.length;
  if (request + response === 0) return t("protection.headers.none");
  return t("protection.headers.summary", { request, response });
}
