import { useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { EmptyState } from "@/components/patterns/empty-state";
import { Inspector, InspectorSection } from "@/components/patterns/inspector";
import { KeyValueGrid } from "@/components/patterns/key-value-grid";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { Button } from "@/components/ui/button";
import { type Status, StatusDot } from "@/components/ui/status-dot";

interface DemoRoute {
  id: string;
  hostname: string;
  origin: string;
  status: Status;
}

const routes: readonly DemoRoute[] = [
  { id: "1", hostname: "app.xyz.com", origin: "localhost:3000", status: "healthy" },
  { id: "2", hostname: "api.xyz.com", origin: "localhost:8080", status: "healthy" },
  { id: "3", hostname: "yx.com", origin: "localhost:5000", status: "connecting" },
  { id: "4", hostname: "docs.yx.com", origin: "localhost:4321", status: "warning" },
  { id: "5", hostname: "admin.xyz.com", origin: "localhost:9000", status: "idle" },
];

/** Sample list │ detail │ inspector layout for the gallery (fake data, dev only). */
export function PatternsDemo() {
  const [selectedId, setSelectedId] = useState<string | null>("1");
  const selected = routes.find((route) => route.id === selectedId);
  return (
    <div className="flex h-[360px] overflow-hidden rounded-card border-hairline border-separator bg-surface-content">
      <SplitView
        id="gallery-demo"
        defaultListWidth={220}
        list={
          <ListPane
            label="Routes"
            items={routes}
            getId={(route) => route.id}
            selectedId={selectedId}
            onSelect={setSelectedId}
            renderRow={(route) => (
              <ListRow
                title={route.hostname}
                subtitle={route.origin}
                leading={<StatusDot status={route.status} />}
              />
            )}
          />
        }
        inspector={
          selected ? (
            <Inspector
              title={selected.hostname}
              subtitle={`Route to ${selected.origin}`}
              actions={<Button size="sm">Open in browser</Button>}
            >
              <InspectorSection title="Public URL">
                <CopyField label="URL" value={`https://${selected.hostname}`} />
              </InspectorSection>
              <InspectorSection title="Details">
                <KeyValueGrid
                  items={[
                    { label: "Origin", value: `http://${selected.origin}`, mono: true },
                    { label: "Tunnel", value: "MacBook" },
                    { label: "DNS", value: "CNAME · proxied" },
                  ]}
                />
              </InspectorSection>
            </Inspector>
          ) : undefined
        }
      >
        {selected ? (
          <div className="flex flex-1 items-center justify-center text-secondary">
            Traffic for {selected.hostname}
          </div>
        ) : (
          <EmptyState title="No route selected" description="Select a route to see its details." />
        )}
      </SplitView>
    </div>
  );
}
