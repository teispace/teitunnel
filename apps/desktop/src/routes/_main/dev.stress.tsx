import { createFileRoute, notFound } from "@tanstack/react-router";
import { lazy } from "react";

// Dev-only, like the gallery: dropped from release builds.
const Stress = __DEV_PAGES__ ? lazy(() => import("@/dev/stress")) : () => null;

export const Route = createFileRoute("/_main/dev/stress")({
  beforeLoad: () => {
    if (!__DEV_PAGES__) throw notFound();
  },
  component: Stress,
});
