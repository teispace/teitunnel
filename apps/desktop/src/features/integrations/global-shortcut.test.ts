import { describe, expect, it } from "vitest";
import { acceleratorFromEvent, formatAccelerator, type KeyEvent } from "./global-shortcut";

const key = (code: string, mods: Partial<KeyEvent> = {}): KeyEvent => ({
  code,
  key: "",
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("acceleratorFromEvent", () => {
  it("reads the platform's primary modifier as CommandOrControl", () => {
    expect(acceleratorFromEvent(key("KeyS", { metaKey: true, shiftKey: true }), "macos")).toBe(
      "CommandOrControl+Shift+S",
    );
    expect(acceleratorFromEvent(key("KeyS", { ctrlKey: true, altKey: true }), "windows")).toBe(
      "CommandOrControl+Alt+S",
    );
    expect(acceleratorFromEvent(key("Space", { ctrlKey: true }), "macos")).toBe("Control+Space");
    expect(acceleratorFromEvent(key("F5", { metaKey: true }), "linux")).toBe("Super+F5");
    expect(acceleratorFromEvent(key("Slash", { altKey: true }), "linux")).toBe("Alt+/");
  });

  it("waits while only modifiers are held", () => {
    expect(acceleratorFromEvent(key("ShiftLeft", { shiftKey: true }), "macos")).toBeNull();
    expect(acceleratorFromEvent(key("MetaLeft", { metaKey: true }), "macos")).toBeNull();
  });
});

describe("formatAccelerator", () => {
  it("draws macOS symbols in Apple's order", () => {
    expect(formatAccelerator("CommandOrControl+Alt+Shift+S", "macos")).toBe("⌥⇧⌘S");
    expect(formatAccelerator("Control+Space", "macos")).toBe("⌃Space");
  });

  it("spells modifiers out elsewhere", () => {
    expect(formatAccelerator("CommandOrControl+Alt+Shift+S", "windows")).toBe("Ctrl+Alt+Shift+S");
    expect(formatAccelerator("Super+F5", "windows")).toBe("Win+F5");
    expect(formatAccelerator("Super+F5", "linux")).toBe("Super+F5");
  });
});
