import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { t } from "@/lib/i18n";
import { commands, type TapScope } from "@/lib/ipc/bindings";
import { call, toIpcError } from "@/lib/ipc/client";
import { queryKeys } from "@/lib/ipc/query-keys";
import { useSetTapComments } from "../queries";

/** The inspector's running taps (a share or inspected route's comments live on its tap). */
function useTaps() {
  return useQuery({
    queryKey: [...queryKeys.inspector.all(), "taps"],
    queryFn: () => call(commands.inspectTaps()),
    staleTime: 5_000,
  });
}

/** What a toggle is for: a Quick Share, or a route (or share on your domain). */
export type CommentsTarget =
  | { kind: "quickShare"; shareId: string }
  | { kind: "route"; accountId: string; hostname: string };

function matches(scope: TapScope, target: CommentsTarget): boolean {
  if (target.kind === "quickShare") {
    return scope.kind === "quickShare" && scope.shareId === target.shareId;
  }
  return (
    scope.kind === "route" &&
    scope.accountId === target.accountId &&
    scope.hostname.toLowerCase() === target.hostname.toLowerCase()
  );
}

/** The comments subject's key for a target (as the core names it). */
export function subjectKey(target: CommentsTarget): string {
  return target.kind === "quickShare"
    ? `share:${target.shareId}`
    : `route:${target.accountId}:${target.hostname.toLowerCase()}`;
}

/**
 * "Comments" on a share or inspected route: reviewers get a small overlay on its pages
 * to pin comments; they're kept on this computer. Shown only while the share goes
 * through the inspector (its tap carries the overlay).
 */
export function CommentsToggle({ target }: { target: CommentsTarget }) {
  const taps = useTaps();
  const set = useSetTapComments();
  const navigate = useNavigate();
  const view = taps.data?.find((candidate) => matches(candidate.scope, target));
  if (!view) return null;
  const tap = view.id;
  const id = `comments-${tap}`;
  return (
    <div className="flex items-center gap-2 text-callout">
      <Switch
        id={id}
        checked={view.comments}
        disabled={set.isPending}
        onCheckedChange={(on) =>
          set.mutate(
            { tap, on },
            {
              onSuccess: () =>
                toast.success(on ? t("comments.toggle.on") : t("comments.toggle.off")),
              onError: (error) => toast.error(toIpcError(error).message),
            },
          )
        }
      />
      <label htmlFor={id} className="text-primary">
        {t("comments.toggle.label")}
      </label>
      {view.comments ? (
        <Button
          size="sm"
          variant="plain"
          className="ml-auto"
          onClick={() =>
            void navigate({ to: "/comments", search: { subject: subjectKey(target) } })
          }
        >
          {t("comments.toggle.open")}
        </Button>
      ) : null}
    </div>
  );
}
