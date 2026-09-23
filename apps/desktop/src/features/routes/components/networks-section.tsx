import { Trash2, TriangleAlert } from "lucide-react";
import { InspectorSection } from "@/components/patterns/inspector";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { useIssues } from "@/features/doctor/queries";
import { useRoutesOverview } from "../queries";

interface NetworksSectionProps {
  accountId: string;
  onAdd: () => void;
  onRemove: (network: string) => void;
}

/**
 * Private networks this Mac's tunnel carries for WARP clients, with the Doctor's findings
 * about each (Split Tunnels, Gateway proxy) inline, since those are why one doesn't work.
 */
export function NetworksSection({ accountId, onAdd, onRemove }: NetworksSectionProps) {
  const overview = useRoutesOverview(accountId);
  const { issues } = useIssues();
  if (!overview.data) return null;
  const networks = overview.data.networks;
  const problems = issues.filter(
    (issue) => issue.accountId === accountId && issue.check.startsWith("network."),
  );
  const general = problems.filter((p) => !networks?.some((n) => n.network === p.subject));

  return (
    <InspectorSection title="Private Networks">
      {networks === null ? (
        <p className="text-callout text-secondary">
          This account's API token can't read private networks. Give it the Cloudflare Tunnel
          permission to share them.
        </p>
      ) : (
        <>
          {networks.length === 0 ? (
            <p className="text-callout text-secondary">
              Let devices running Cloudflare WARP reach addresses on this Mac's network, like a NAS
              or a printer.
            </p>
          ) : (
            <ul
              aria-label="Private networks"
              className="flex flex-col rounded-card bg-surface-inset px-3 py-1"
            >
              {networks.map((network) => {
                const problem = problems.find((p) => p.subject === network.network);
                return (
                  <li
                    key={network.network}
                    className="flex min-h-9 items-center gap-2.5 border-inset border-b-hairline py-1.5 last:border-b-0"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="selectable truncate font-mono text-mono">
                          {network.network}
                        </span>
                        {network.private ? null : <Badge>Public</Badge>}
                      </div>
                      {problem ? (
                        <p className="flex items-center gap-1 text-callout text-warning">
                          <TriangleAlert aria-hidden className="size-3 shrink-0" strokeWidth={2} />
                          {problem.title}
                        </p>
                      ) : network.owned ? null : (
                        <p className="text-callout text-secondary">Added outside Teitunnel</p>
                      )}
                    </div>
                    <IconButton
                      icon={Trash2}
                      label={`Stop sharing ${network.network}`}
                      variant="secondary"
                      onClick={() => onRemove(network.network)}
                    />
                  </li>
                );
              })}
            </ul>
          )}
          {general.map((problem) => (
            <p key={problem.id} className="flex items-start gap-1.5 text-callout text-warning">
              <TriangleAlert aria-hidden className="mt-0.5 size-3 shrink-0" strokeWidth={2} />
              <span>
                {problem.title}. <span className="text-secondary">{problem.detail}</span>
              </span>
            </p>
          ))}
          <div>
            <Button onClick={onAdd}>Share Network…</Button>
          </div>
        </>
      )}
    </InspectorSection>
  );
}
