import { createFileRoute, notFound } from "@tanstack/react-router";
import { lazy } from "react";

// Dev-only, like the gallery: dropped from release builds.
const Stress = import.meta.env.DEV ? lazy(() => import("@/dev/stress")) : () => null;

export const Route = createFileRoute("/_main/dev/stress")({
  beforeLoad: () => {
    if (!import.meta.env.DEV) throw notFound();
  },
  component: Stress,
});
