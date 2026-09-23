import type { Platform } from "./platform";

/**
 * A keyboard shortcut with the platform's primary modifier: ⌘ on macOS, Ctrl on
 * Windows and Linux. `key` is a letter, a digit or `,`.
 */
export interface Shortcut {
  key: string;
  shift?: boolean;
  alt?: boolean;
}

/** How menus show it: `⌥⌘S` on macOS (modifiers in Apple's order), `Ctrl+Alt+S` elsewhere. */
export function formatShortcut(shortcut: Shortcut, platform: Platform): string {
  if (platform === "macos") {
    return `${shortcut.alt ? "⌥" : ""}${shortcut.shift ? "⇧" : ""}⌘${shortcut.key}`;
  }
  return ["Ctrl", shortcut.shift && "Shift", shortcut.alt && "Alt", shortcut.key]
    .filter(Boolean)
    .join("+");
}

/** The physical key (`event.code`), so Alt or another layout doesn't change the match. */
function codeOf(key: string): string {
  if (/^[A-Z]$/.test(key)) return `Key${key}`;
  if (/^[0-9]$/.test(key)) return `Digit${key}`;
  return key === "," ? "Comma" : key;
}

/** Whether `event` is `shortcut` on `platform` (and no other modifier is held). */
export function matchesShortcut(
  event: Pick<KeyboardEvent, "code" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">,
  shortcut: Shortcut,
  platform: Platform,
): boolean {
  const mac = platform === "macos";
  const primary = mac ? event.metaKey : event.ctrlKey;
  const other = mac ? event.ctrlKey : event.metaKey;
  return (
    primary &&
    !other &&
    event.shiftKey === Boolean(shortcut.shift) &&
    event.altKey === Boolean(shortcut.alt) &&
    event.code === codeOf(shortcut.key)
  );
}
