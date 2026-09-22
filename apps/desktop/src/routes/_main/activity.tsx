import { createFileRoute } from "@tanstack/react-router";
import { Activity } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";

export const Route = createFileRoute("/_main/activity")({
  component: ActivityPage,
});

function ActivityPage() {
  return (
    <>
      <TitlebarToolbar title="Activity" />
      <EmptyState
        icon={Activity}
        title="No activity yet"
        description="Every change Teitunnel makes to Cloudflare is recorded here, step by step."
      />
    </>
  );
}
