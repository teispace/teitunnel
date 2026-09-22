import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { events } from "@/lib/ipc/bindings";
import { runMenuCommand } from "./commands";
import { isTauri } from "./platform";
import { useUiStore } from "./ui-store";

/** Routes menu-bar actions from Rust to app commands. Mount once inside the router. */
export function MenuBridge() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  useEffect(() => {
    if (!isTauri()) {
      // Outside the app (tests, WebKit screenshots) there's no menu bar to own ⌘K.
      const onKeyDown = (event: KeyboardEvent) => {
        if (event.metaKey && event.key === "k") useUiStore.getState().setPaletteOpen(true);
      };
      window.addEventListener("keydown", onKeyDown);
      return () => window.removeEventListener("keydown", onKeyDown);
    }
    const unlisten = events.menuAction.listen(({ payload }) => {
      if (payload.command === "commandPalette") {
        useUiStore.getState().setPaletteOpen(true);
        return;
      }
      runMenuCommand(payload.command, { navigate, queryClient });
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [navigate, queryClient]);
  return null;
}
