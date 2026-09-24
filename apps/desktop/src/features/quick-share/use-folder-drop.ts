import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";
import { isTauri } from "@/app/platform";

/**
 * Folders dropped on the window (the native drop: the webview gets paths, which a web
 * drop never has). Returns whether something is being dragged over the window, so the
 * page can show where it will land.
 */
export function useFolderDrop(onDrop: (path: string) => void): boolean {
  const [over, setOver] = useState(false);
  const latest = useRef(onDrop);
  latest.current = onDrop;
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setOver(true);
        } else if (payload.type === "leave") {
          setOver(false);
        } else {
          setOver(false);
          const first = payload.paths[0];
          if (first) latest.current(first);
        }
      })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  return over;
}
