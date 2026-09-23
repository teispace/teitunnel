import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { MotionConfig } from "motion/react";
import { useEffect, useState } from "react";
import { Toaster } from "@/components/ui/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { settingsQuery } from "@/features/settings";
import { commands } from "@/lib/ipc/bindings";
import { syncEntityChanges } from "@/lib/ipc/events";
import { isTauri, syncWindowChrome } from "./platform";
import { createQueryClient } from "./query-client";
import { createAppRouter } from "./router";
import { applyTheme } from "./theme";
import { ThemeSync } from "./theme-sync";

export function App() {
  const [queryClient] = useState(createQueryClient);
  const [router] = useState(() => createAppRouter(queryClient));

  useEffect(() => syncWindowChrome(), []);
  useEffect(() => (isTauri() ? syncEntityChanges(queryClient) : undefined), [queryClient]);
  useEffect(() => {
    if (!isTauri()) return;
    // Apply the saved appearance before the (hidden) window is shown, so it never
    // flashes the wrong theme.
    void queryClient
      .fetchQuery(settingsQuery)
      .then((settings) => applyTheme(settings.theme))
      .catch((error: unknown) => console.warn("could not load settings", error))
      .finally(signalReady);
  }, [queryClient]);

  return (
    <QueryClientProvider client={queryClient}>
      {/* motion ignores Reduce Motion unless told to (its default is "never"). */}
      <MotionConfig reducedMotion="user">
        <TooltipProvider>
          <RouterProvider router={router} />
          <ThemeSync />
          <Toaster />
        </TooltipProvider>
      </MotionConfig>
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
