import { Info, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { t, translate } from "@/lib/i18n";
import type { LocalDomainsStatus, PortProblem } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useLocalDomains, useRestart, useRunAsAdmin } from "../queries";

/** A problem with its fix, in place. */
export function Callout({
  tone = "warning",
  title,
  children,
  actions,
}: {
  tone?: "warning" | "info";
  title: string;
  children: ReactNode;
  actions?: ReactNode;
}) {
  const Icon = tone === "warning" ? TriangleAlert : Info;
  return (
    <section aria-label={title} className="flex gap-3 rounded-card bg-surface-inset p-4 text-left">
      <Icon
        aria-hidden
        className={`mt-0.5 size-4 shrink-0 ${tone === "warning" ? "text-warning" : "text-secondary"}`}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <h3 className="text-headline">{title}</h3>
        {children}
        {actions ? <div className="flex flex-wrap gap-2">{actions}</div> : null}
      </div>
    </section>
  );
}

/** Browsers don't trust the local CA yet: one button to the walkthrough. */
export function TrustCallout({ onSetUp }: { onSetUp: () => void }) {
  return (
    <Callout
      title={t("localDomains.callout.untrusted")}
      actions={
        <Button variant="primary" size="sm" onClick={onSetUp}>
          {t("localDomains.callout.setUpTrust")}
        </Button>
      }
    >
      <p className="text-callout text-secondary">{t("localDomains.callout.untrustedDetail")}</p>
    </Callout>
  );
}

/**
 * `.test` names don't reach Teitunnel yet: the one-time command (it needs an
 * administrator), and on Linux a button that runs it. Checked again every few seconds
 * while shown, so it goes away by itself once done.
 */
export function ResolverFix({ status }: { status: LocalDomainsStatus }) {
  useLocalDomains({ recheck: true });
  const admin = useRunAsAdmin();
  const restart = useRestart();
  const resolver = status.resolver;
  if (resolver.error) {
    return (
      <Callout
        title={t("localDomains.callout.dnsStopped")}
        actions={
          <Button size="sm" onClick={() => restart.mutate()} pending={restart.isPending}>
            {t("localDomains.tryAgain")}
          </Button>
        }
      >
        <p className="text-callout text-secondary">{translate(resolver.error)}</p>
      </Callout>
    );
  }
  return (
    <Callout
      title={t("localDomains.callout.resolver")}
      actions={
        status.platform === "linux" ? (
          <Button size="sm" onClick={() => admin.mutate("resolver")} pending={admin.isPending}>
            {t("localDomains.runAsAdmin")}
          </Button>
        ) : null
      }
    >
      <p className="text-callout text-secondary">
        {t("localDomains.callout.resolverDetail", { port: String(resolver.port) })}
      </p>
      <CopyField
        multiline
        label={t("localDomains.callout.copyCommand")}
        value={resolver.setup.map((s) => s.command).join("\n")}
      />
      <p className="text-footnote text-secondary" role="status">
        {t("localDomains.callout.waiting")}
      </p>
      {admin.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(admin.error).message}
        </p>
      ) : null}
    </Callout>
  );
}

function portText(problem: PortProblem): string {
  const reason = t(`localDomains.port.${problem.reason}`, { port: String(problem.port) });
  return problem.fallback === null
    ? t("localDomains.port.none", { reason })
    : t("localDomains.port.fallback", { fallback: String(problem.fallback), reason });
}

/** Local domains aren't served, or not on the usual ports: why, and Try Again. */
export function ServingNotice({ status }: { status: LocalDomainsStatus }) {
  const restart = useRestart();
  const problems = status.portProblems;
  if (!status.error && problems.length === 0) return null;
  const failed = status.error !== null || problems.some((p) => p.fallback === null);
  return (
    <Callout
      tone={failed ? "warning" : "info"}
      title={failed ? t("localDomains.callout.stopped") : t("localDomains.callout.otherPort")}
      actions={
        <Button size="sm" onClick={() => restart.mutate()} pending={restart.isPending}>
          {t("localDomains.tryAgain")}
        </Button>
      }
    >
      {status.error ? (
        <p className="text-callout text-secondary">{translate(status.error)}</p>
      ) : null}
      {problems.map((problem) => (
        <p key={problem.port} className="text-callout text-secondary">
          {portText(problem)}
        </p>
      ))}
    </Callout>
  );
}
