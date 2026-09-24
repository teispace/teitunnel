import type { useNavigate } from "@tanstack/react-router";
import type { ViewTarget } from "@/lib/ipc/bindings";

/**
 * Shows the view the control connection or a `teitunnel://` link asked for; a share's
 * inspector opens the Inspector on that share (its tap has the share's id).
 */
export function openView(target: ViewTarget, navigate: ReturnType<typeof useNavigate>) {
  switch (target.view) {
    case "overview":
      return navigate({ to: "/" });
    case "route":
      return navigate({ to: "/routes", search: { route: target.hostname } });
    case "share":
      return navigate({ to: "/quick-share" });
    case "inspector":
      return navigate({ to: "/inspector", search: { tap: target.share } });
    case "doctor":
      return navigate({ to: "/doctor" });
  }
}
