import { createFileRoute } from "@tanstack/react-router";
import { QuickSharePage } from "@/features/quick-share";

export const Route = createFileRoute("/_main/quick-share")({
  component: QuickSharePage,
});
