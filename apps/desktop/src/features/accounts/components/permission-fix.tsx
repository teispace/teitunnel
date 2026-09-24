import { ExternalLink, KeyRound } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { t } from "@/lib/i18n";
import type { Capabilities } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { openTokenPage, useAccounts, useAddToken, useCapabilities } from "../queries";

/** Something a feature needs the account's credential to be allowed to do. */
export type PermissionNeed =
  | { kind: "tunnels" }
  | { kind: "zones" }
  | { kind: "dns"; zone: string }
  /** DNS on at least one domain (routes need one to be useful at all). */
  | { kind: "anyDns" }
  /** Cloudflare Load Balancing (a paid add-on); not probed, so shown only on refusal. */
  | { kind: "loadBalancing" }
  | { kind: "access" };

/** The needs a check found missing ("unknown" isn't: it may just be offline). */
export function missingNeeds(caps: Capabilities, needs: PermissionNeed[]): PermissionNeed[] {
  return needs.filter((need) => isMissing(caps, need));
}

/** Whether a check found the need missing ("unknown" isn't: it may just be offline). */
function isMissing(caps: Capabilities, need: PermissionNeed): boolean {
  switch (need.kind) {
    case "tunnels":
      return caps.tunnelsEdit === "no";
    case "zones":
      return caps.zonesRead === "no";
    case "dns":
      return caps.zones.some((z) => z.zoneName === need.zone && z.dnsEdit === "no");
    case "anyDns":
      return caps.zones.length > 0 && caps.zones.every((z) => z.dnsEdit === "no");
    case "loadBalancing":
      return false;
    case "access":
      return caps.accessEdit === "no";
  }
}

/** The dashboard's permission names for a need, with why Teitunnel needs each. */
function permissions(need: PermissionNeed): { name: string; why: string }[] {
  switch (need.kind) {
    case "tunnels":
      return [{ name: t("permissionFix.tunnels"), why: t("permissionFix.tunnelsWhy") }];
    case "zones":
      return [{ name: t("permissionFix.zones"), why: t("permissionFix.zonesWhy") }];
    case "dns":
      return [
        {
          name: t("permissionFix.dns"),
          why: t("permissionFix.dnsWhy", { zone: need.zone }),
        },
      ];
    case "anyDns":
      return [{ name: t("permissionFix.dns"), why: t("permissionFix.anyDnsWhy") }];
    case "loadBalancing":
      return [
        { name: t("permissionFix.lbPools"), why: t("permissionFix.lbWhy") },
        { name: t("permissionFix.lbBalancers"), why: t("permissionFix.lbWhy") },
      ];
    case "access":
      return [
        { name: t("permissionFix.accessApps"), why: t("permissionFix.accessAppsWhy") },
        { name: t("permissionFix.accessOrg"), why: t("permissionFix.accessOrgWhy") },
      ];
  }
}

/**
 * Replaces the account's credential with a new token that has every permission. Not a
 * `<form>`: it's shown inside other forms, and forms can't nest.
 */
function NewToken({ detail, onConnected }: { detail: string; onConnected: () => void }) {
  const [token, setToken] = useState("");
  const addToken = useAddToken();
  const canSubmit = token.trim() !== "" && !addToken.isPending;
  const submit = () => {
    if (!canSubmit) return;
    addToken.mutate(token, {
      onSuccess: () => {
        setToken("");
        onConnected();
      },
    });
  };
  return (
    <div className="flex flex-col gap-2">
      <p className="text-callout text-secondary">{detail}</p>
      <div>
        <Button size="sm" onClick={() => void openTokenPage("create")}>
          {t("permissionFix.createToken")} <ExternalLink />
        </Button>
      </div>
      <Field
        label={t("connect.token")}
        error={addToken.error ? toIpcError(addToken.error).message : null}
      >
        {(control) => (
          <Input
            {...control}
            type="password"
            autoComplete="off"
            placeholder={t("connect.tokenPlaceholder")}
            value={token}
            onChange={(event) => {
              setToken(event.target.value);
              if (addToken.error) addToken.reset();
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter") return;
              // Keep Enter from submitting a form around this one.
              event.preventDefault();
              submit();
            }}
          />
        )}
      </Field>
      <div>
        <Button
          size="sm"
          variant="primary"
          disabled={!canSubmit}
          pending={addToken.isPending}
          onClick={submit}
        >
          {addToken.isPending ? t("connect.checking") : t("connect.connect")}
        </Button>
      </div>
    </div>
  );
}

interface PermissionFixProps {
  accountId: string;
  /** What the feature at hand needs. */
  needs: PermissionNeed[];
  /**
   * Cloudflare already refused: show the fix even if an older check said allowed (then
   * every need is listed, since the check can't tell which one is missing).
   */
  refused?: boolean;
  /** Called when a re-check finds the permissions, e.g. to review the change again. */
  onReady?: () => void;
}

/**
 * Shown where a feature needs a permission the account's credential lacks. It lists
 * what to add to the token (which keeps its value, so nothing is pasted) and checks
 * again whenever the window regains focus, so coming back from the browser is enough.
 */
export function PermissionFix({ accountId, needs, refused = false, onReady }: PermissionFixProps) {
  const account = useAccounts().data?.find((a) => a.id === accountId);
  const caps = useCapabilities(accountId);
  const [checked, setChecked] = useState(false);
  const [newToken, setNewToken] = useState(false);
  const { refetch } = caps;

  // Only an explicit re-check (focus, Check Again, a new token) reports readiness, so a
  // refusal that persists can't loop through onReady.
  const recheck = useCallback(async () => {
    const { data } = await refetch();
    setChecked(true);
    if (!data || needs.some((need) => isMissing(data, need))) return;
    toast.success(t("permissionFix.ready"));
    onReady?.();
  }, [refetch, onReady, needs]);

  useEffect(() => {
    const onFocus = () => void recheck();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [recheck]);

  // Wait for both, so the right path shows from the first frame.
  if (!account || caps.isPending) return null;
  const found = caps.data ? needs.filter((need) => isMissing(caps.data, need)) : [];
  const shown = found.length > 0 ? found : refused ? needs : [];
  if (shown.length === 0) return null;
  const list = shown.flatMap(permissions);
  const token = account.credential === "apiToken";
  const title = t("permissionFix.title", { count: list.length });

  return (
    <section aria-label={title} className="flex gap-3 rounded-card bg-surface-inset p-4">
      <KeyRound aria-hidden className="mt-0.5 size-4 shrink-0 text-warning" />
      <div className="flex min-w-0 flex-1 flex-col gap-3">
        <h3 className="text-headline">{title}</h3>
        <ul className="flex flex-col gap-1 text-callout">
          {list.map(({ name, why }) => (
            <li key={`${name} ${why}`}>
              <span className="font-semibold">{name}</span>
              <span className="block text-secondary">{why}</span>
            </li>
          ))}
        </ul>
        {token && !newToken ? (
          <>
            <p className="text-callout text-secondary">{t("permissionFix.editDetail")}</p>
            <ol className="flex list-decimal flex-col gap-1 pl-5 text-callout">
              <li>{t("permissionFix.step1")}</li>
              <li>{t("permissionFix.step2")}</li>
              <li>{t("permissionFix.step3")}</li>
            </ol>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="primary" onClick={() => void openTokenPage("edit")}>
                {t("permissionFix.openTokens")} <ExternalLink />
              </Button>
              <Button size="sm" disabled={caps.isFetching} onClick={() => void recheck()}>
                {caps.isFetching ? (
                  <>
                    <Spinner className="size-3" /> {t("permissionFix.checking")}
                  </>
                ) : (
                  t("permissionFix.checkAgain")
                )}
              </Button>
            </div>
            <p aria-live="polite" className="text-footnote text-secondary">
              {checked && !caps.isFetching && found.length > 0
                ? t("permissionFix.stillMissing")
                : t("permissionFix.autoCheck")}
            </p>
            <div className="-mt-1">
              <Button
                size="sm"
                variant="plain"
                className="-ml-2.5"
                onClick={() => setNewToken(true)}
              >
                {t("permissionFix.useNewToken")}
              </Button>
            </div>
          </>
        ) : (
          <NewToken
            detail={token ? t("permissionFix.newTokenDetail") : t("permissionFix.otherDetail")}
            onConnected={() => void recheck()}
          />
        )}
      </div>
    </section>
  );
}
