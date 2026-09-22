import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { Toaster } from "@/components/ui/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { commands } from "@/lib/ipc/bindings";
import { syncEntityChanges } from "@/lib/ipc/events";
import { isTauri, syncWindowChrome } from "./platform";
import { createQueryClient } from "./query-client";
import { createAppRouter } from "./router";

export function App() {
  const [queryClient] = useState(createQueryClient);
  const [router] = useState(() => createAppRouter(queryClient));

  useEffect(() => syncWindowChrome(), []);
  useEffect(() => (isTauri() ? syncEntityChanges(queryClient) : undefined), [queryClient]);
  useEffect(() => {
    if (isTauri()) signalReady();
  }, []);

  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <RouterProvider router={router} />
        <Toaster />
      </TooltipProvider>
    </QueryClientProvider>
  );
}

/**
 * Tells Rust the first frame is ready so it can show the (hidden) window without a
 * white flash. rAF can be throttled while the window is hidden, so a timeout races it.
 */
function signalReady() {
  let sent = false;
  const send = () => {
    if (sent) return;
    sent = true;
    void commands.appReady();
  };
  requestAnimationFrame(() => requestAnimationFrame(send));
  setTimeout(send, 50);
}
