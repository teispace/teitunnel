import { createFileRoute } from "@tanstack/react-router";
import { LayoutGrid } from "lucide-react";
import { EmptyState } from "@/components/patterns/empty-state";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { useAppInfo } from "@/features/app";

export const Route = createFileRoute("/_main/")({
  component: Overview,
});

function Overview() {
  const { data: info } = useAppInfo();
  return (
    <>
      <TitlebarToolbar title="Overview" />
      <EmptyState
        icon={LayoutGrid}
        title="Nothing running yet"
        description="Routes and Quick Shares you start will appear here with their live status."
      />
      {info ? (
        <p className="selectable pointer-events-auto absolute right-4 bottom-3 text-footnote text-tertiary tabular">
          Teitunnel {info.version} · {info.platform} {info.arch}
        </p>
      ) : null}
    </>
  );
}
