import { describe, expect, it } from "vitest";
import { formatShortcut, matchesShortcut } from "./shortcuts";

const press = (
  code: string,
  mods: Partial<Record<"meta" | "ctrl" | "shift" | "alt", boolean>>,
) => ({
  code,
  metaKey: mods.meta ?? false,
  ctrlKey: mods.ctrl ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
});

describe("shortcuts", () => {
  it("shows each platform's convention", () => {
    expect(formatShortcut({ key: "N", shift: true }, "macos")).toBe("⇧⌘N");
    expect(formatShortcut({ key: "S", alt: true }, "macos")).toBe("⌥⌘S");
    expect(formatShortcut({ key: "N", shift: true }, "windows")).toBe("Ctrl+Shift+N");
    expect(formatShortcut({ key: "1" }, "linux")).toBe("Ctrl+1");
  });

  it("matches the primary modifier and exactly the others", () => {
    const n = { key: "N" };
    expect(matchesShortcut(press("KeyN", { ctrl: true }), n, "windows")).toBe(true);
    expect(matchesShortcut(press("KeyN", { meta: true }), n, "macos")).toBe(true);
    expect(matchesShortcut(press("KeyN", { meta: true }), n, "windows")).toBe(false);
    expect(matchesShortcut(press("KeyN", { ctrl: true, shift: true }), n, "windows")).toBe(false);
    // Alt changes the character on some layouts; the physical key still matches.
    expect(
      matchesShortcut(press("KeyS", { meta: true, alt: true }), { key: "S", alt: true }, "macos"),
    ).toBe(true);
    expect(matchesShortcut(press("Comma", { ctrl: true }), { key: "," }, "linux")).toBe(true);
    expect(matchesShortcut(press("Digit3", { ctrl: true }), { key: "3" }, "linux")).toBe(true);
  });
});
