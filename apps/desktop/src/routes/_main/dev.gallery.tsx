import { createFileRoute, notFound } from "@tanstack/react-router";
import { lazy } from "react";

// The gallery module is only referenced in dev builds, so it is dropped from releases.
const Gallery = import.meta.env.DEV ? lazy(() => import("@/dev/gallery")) : () => null;

export const Route = createFileRoute("/_main/dev/gallery")({
  beforeLoad: () => {
    if (!import.meta.env.DEV) throw notFound();
  },
  component: Gallery,
});
