import type { Platform } from "@/app/platform";

/** The keyboard event fields a recorded shortcut is read from. */
export type KeyEvent = Pick<
  KeyboardEvent,
  "code" | "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"
>;

const NAMED: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Insert: "Insert",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backquote: "`",
};

/** The key part of an accelerator from the physical key, or null for a modifier. */
function keyOf(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter?.[1]) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit?.[1]) return digit[1];
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return NAMED[code] ?? null;
}

/**
 * The shortcut being typed, in Tauri's accelerator syntax (`CommandOrControl+Alt+S`),
 * or null while only modifiers are held. ⌘ on macOS and Ctrl elsewhere become
 * `CommandOrControl`, so a shortcut saved on one platform means the same elsewhere.
 */
export function acceleratorFromEvent(event: KeyEvent, platform: Platform): string | null {
  const key = keyOf(event.code);
  if (!key) return null;
  const mac = platform === "macos";
  const parts: string[] = [];
  if (mac ? event.metaKey : event.ctrlKey) parts.push("CommandOrControl");
  if (mac ? event.ctrlKey : event.metaKey) parts.push(mac ? "Control" : "Super");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

const MAC_SYMBOLS: Record<string, string> = {
  Control: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  CommandOrControl: "⌘",
  Command: "⌘",
  Super: "⌘",
};

const MAC_ORDER = ["Control", "Alt", "Shift", "CommandOrControl", "Command", "Super"];

const MAC_KEYS: Record<string, string> = {
  Space: "Space",
  Enter: "↩",
  Tab: "⇥",
  Backspace: "⌫",
  Delete: "⌦",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
};

const OTHER_NAMES: Record<string, string> = {
  CommandOrControl: "Ctrl",
  Control: "Ctrl",
  Alt: "Alt",
  AltGr: "AltGr",
  Shift: "Shift",
};

/** How the shortcut reads: `⌥⇧⌘S` on macOS (Apple's order), `Ctrl+Alt+Shift+S` elsewhere. */
export function formatAccelerator(keys: string, platform: Platform): string {
  const parts = keys.split("+");
  const key = parts.pop() ?? "";
  if (platform === "macos") {
    const modifiers = MAC_ORDER.filter((m) => parts.includes(m))
      .map((m) => MAC_SYMBOLS[m])
      .join("");
    return `${modifiers}${MAC_KEYS[key] ?? key}`;
  }
  const logo = platform === "linux" ? "Super" : "Win";
  const names = parts.map((m) => (m === "Super" || m === "Command" ? logo : (OTHER_NAMES[m] ?? m)));
  return [...names, key].join("+");
}
