import { useQueryClient } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { queryKeys } from "@/lib/ipc/query-keys";

/**
 * Shown when cloudflared isn't installed. One-click managed install arrives with
 * M1-02; until then, Homebrew is the recommended route.
 */
export function BinaryNotice() {
  const queryClient = useQueryClient();
  return (
    <section
      aria-label="cloudflared isn't installed"
      className="flex gap-3 rounded-card bg-surface-inset p-4"
    >
      <TriangleAlert
        aria-hidden
        className="mt-0.5 size-4 shrink-0 text-warning"
        strokeWidth={1.75}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div>
          <h2 className="text-headline">cloudflared isn't installed</h2>
          <p className="mt-0.5 text-callout text-secondary">
            Teitunnel uses cloudflared, Cloudflare's connector, to share services. Install it with
            Homebrew, then check again.
          </p>
        </div>
        <CopyField label="command" value="brew install cloudflared" className="max-w-80" />
        <div>
          <Button
            size="sm"
            onClick={() =>
              void queryClient.invalidateQueries({ queryKey: queryKeys.binary.status() })
            }
          >
            Check again
          </Button>
        </div>
      </div>
    </section>
  );
}
