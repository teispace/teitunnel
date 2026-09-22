import { createFileRoute } from "@tanstack/react-router";
import { Share } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";

export const Route = createFileRoute("/_main/quick-share")({
  component: QuickSharePage,
});

function QuickSharePage() {
  return (
    <>
      <TitlebarToolbar title="Quick Share" />
      <EmptyState
        icon={Share}
        title="Share a local service"
        description="Get a temporary public URL for any local port. No account or domain needed."
      />
    </>
  );
}
