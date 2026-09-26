import { createFileRoute, notFound } from "@tanstack/react-router";
import { lazy } from "react";

// The gallery module is only referenced in dev builds, so it is dropped from releases.
const Gallery = __DEV_PAGES__ ? lazy(() => import("@/dev/gallery")) : () => null;

export const Route = createFileRoute("/_main/dev/gallery")({
  beforeLoad: () => {
    if (!__DEV_PAGES__) throw notFound();
  },
  component: Gallery,
});
