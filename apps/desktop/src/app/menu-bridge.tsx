import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { events } from "@/lib/ipc/bindings";
import { type AppCommand, type CommandContext, commandsFor, runMenuCommand } from "./commands";
import { detectPlatform, isTauri, type Platform } from "./platform";
import { matchesShortcut } from "./shortcuts";
import { useUiStore } from "./ui-store";

/** Runs a command when its shortcut is pressed; returns the cleanup. */
function listenForShortcuts(
  commands: readonly AppCommand[],
  platform: Platform,
  context: CommandContext,
): () => void {
  const onKeyDown = (event: KeyboardEvent) => {
    const command = commands.find(
      (c) => c.shortcut && matchesShortcut(event, c.shortcut, platform),
    );
    if (!command) return;
    // Also keeps the webview's own Ctrl+R (reload) and Ctrl+N (new window) away.
    event.preventDefault();
    command.run(context);
  };
  window.addEventListener("keydown", onKeyDown);
  return () => window.removeEventListener("keydown", onKeyDown);
}

/** Routes menu-bar actions from Rust, and keyboard shortcuts, to app commands. Mount once. */
export function MenuBridge() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  useEffect(() => {
    const platform = detectPlatform();
    // macOS's menu bar owns the shortcuts there. Windows and Linux have none (D-063),
    // and a plain browser (tests, screenshots) neither.
    const stopKeys =
      platform !== "macos" || !isTauri()
        ? listenForShortcuts(commandsFor(platform), platform, { navigate, queryClient })
        : () => {};
    if (!isTauri()) return stopKeys;
    const unlisten = events.menuAction.listen(({ payload }) => {
      if (payload.command === "commandPalette") {
        useUiStore.getState().setPaletteOpen(true);
        return;
      }
      if (payload.command === "confirmQuit") {
        useUiStore.getState().setQuitOpen(true);
        return;
      }
      runMenuCommand(payload.command, { navigate, queryClient });
    });
    return () => {
      stopKeys();
      void unlisten.then((stop) => stop());
    };
  }, [navigate, queryClient]);
  return null;
}
