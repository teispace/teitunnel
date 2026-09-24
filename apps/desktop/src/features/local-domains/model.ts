import type { Status } from "@/components/ui/status-dot";
import { t } from "@/lib/i18n";
import type { LocalDomainView, LocalTarget } from "@/lib/ipc/bindings";

export interface DomainState {
  dot: Status;
  label: string;
}

/**
 * How a domain is doing, most urgent first: not served, not reachable by name, not
 * trusted by browsers yet. `trusted` is unknown (`undefined`) until trust was checked.
 */
export function domainState(domain: LocalDomainView, trusted: boolean | undefined): DomainState {
  if (!domain.serving) return { dot: "error", label: t("localDomains.state.stopped") };
  if (domain.resolution === "needsResolver") {
    return { dot: "warning", label: t("localDomains.state.needsResolver") };
  }
  if (domain.resolution === "elsewhere") {
    return { dot: "warning", label: t("localDomains.state.elsewhere") };
  }
  if (domain.https && trusted === false) {
    return { dot: "warning", label: t("localDomains.state.untrusted") };
  }
  return { dot: "healthy", label: t("localDomains.state.serving") };
}

/** What a target shows as (the service people typed). */
export function targetText(target: LocalTarget, origin: string | null): string {
  switch (target.kind) {
    case "port":
      return `localhost:${target.port}`;
    case "url":
      return target.url;
    case "share":
    case "route":
      return origin ?? target.id;
  }
}

/** The text to edit a target with (a port stays a port). */
export function targetInput(target: LocalTarget): string {
  switch (target.kind) {
    case "port":
      return String(target.port);
    case "url":
      return target.url;
    case "share":
    case "route":
      return target.id;
  }
}

const SUFFIXES = [".localhost", ".test", ".local"] as const;

/** The name as typed, lowercased, with `.localhost` added when no local suffix was typed. */
export function completeName(input: string): string {
  const name = input.trim().toLowerCase().replace(/\.$/, "");
  if (name === "" || SUFFIXES.some((suffix) => name.endsWith(suffix))) return name;
  return `${name}.localhost`;
}

/** The suffix of a local name, if it has one of the three. */
export function suffixOf(name: string): "localhost" | "test" | "local" | null {
  const found = SUFFIXES.find((suffix) => name.endsWith(suffix));
  return found ? (found.slice(1) as "localhost" | "test" | "local") : null;
}
