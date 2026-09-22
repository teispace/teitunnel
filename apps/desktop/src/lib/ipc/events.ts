import type { QueryClient } from "@tanstack/react-query";
import { events } from "./bindings";
import { keysForEntity } from "./query-keys";

/**
 * Invalidates cached queries whenever Rust reports a change. This is the only sync
 * mechanism between the core and the UI; nothing mirrors server state by hand.
 * Returns a function that stops listening.
 */
export function syncEntityChanges(queryClient: QueryClient): () => void {
  const unlisten = events.entityChanged.listen(({ payload }) => {
    for (const queryKey of keysForEntity(payload.kind)) {
      void queryClient.invalidateQueries({ queryKey });
    }
  });
  return () => {
    void unlisten.then((stop) => stop());
  };
}
