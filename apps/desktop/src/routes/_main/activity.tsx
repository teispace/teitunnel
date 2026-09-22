import { createFileRoute } from "@tanstack/react-router";
import { ActivityPage } from "@/features/activity";

export const Route = createFileRoute("/_main/activity")({
  component: ActivityPage,
});
