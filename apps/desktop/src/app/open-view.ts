import type { useNavigate } from "@tanstack/react-router";
import type { ViewTarget } from "@/lib/ipc/bindings";

/**
 * Shows the view the control connection or a `teitunnel://` link asked for. A share's
 * inspector opens Quick Share until the inspector has its own view.
 */
export function openView(target: ViewTarget, navigate: ReturnType<typeof useNavigate>) {
  switch (target.view) {
    case "overview":
      return navigate({ to: "/" });
    case "route":
      return navigate({ to: "/routes", search: { route: target.hostname } });
    case "share":
    case "inspector":
      return navigate({ to: "/quick-share" });
    case "doctor":
      return navigate({ to: "/doctor" });
  }
}
