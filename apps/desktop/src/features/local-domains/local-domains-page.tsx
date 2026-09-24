import { LockKeyhole, Plus, RefreshCw, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { EmptyState } from "@/components/patterns/empty-state";
import { ErrorState } from "@/components/patterns/error-state";
import { ListPane, ListRow } from "@/components/patterns/list-pane";
import { SplitView } from "@/components/patterns/split-view";
import { TitlebarToolbar } from "@/components/patterns/titlebar-toolbar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Skeleton } from "@/components/ui/skeleton";
import { StatusDot } from "@/components/ui/status-dot";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useManualRefetch } from "@/lib/use-manual-refetch";
import { ResolverFix, ServingNotice, TrustCallout } from "./components/callouts";
import { DomainDetail } from "./components/domain-detail";
import { DomainSheet } from "./components/domain-sheet";
import { TrustSheet } from "./components/trust-sheet";
import { domainState, targetText } from "./model";
import { useLocalDomains, useTrust } from "./queries";

/**
 * Local HTTPS domains (`https://shop.test`): its own view rather than a part of Domains,
 * which lists a Cloudflare account's zones and needs an account; these live only on this
 * computer and work without one.
 */
export function LocalDomainsPage() {
  const status = useLocalDomains();
  const domains = status.data?.domains ?? [];
  const https = domains.some((d) => d.https);
  const trust = useTrust({ enabled: https });
  const trusted = https ? trust.data?.trusted : undefined;
  const reload = useManualRefetch(async () => {
    await Promise.all([status.refetch(), https ? trust.refetch() : null]);
  });
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [trusting, setTrusting] = useState(false);
  const current = domains.find((d) => d.name === selected) ?? domains[0] ?? null;
  const resolverNeeded =
    status.data !== undefined &&
    (status.data.resolver.error !== null || domains.some((d) => d.resolution === "needsResolver"));

  const toolbar = (
    <TitlebarToolbar title={t("localDomains.title")}>
      <IconButton
        icon={ShieldCheck}
        label={t("localDomains.trustButton")}
        onClick={() => setTrusting(true)}
      />
      <IconButton
        icon={RefreshCw}
        label={t("localDomains.refresh")}
        onClick={reload.refresh}
        pending={reload.refreshing}
      />
      <IconButton icon={Plus} label={t("localDomains.add")} onClick={() => setAdding(true)} />
    </TitlebarToolbar>
  );
  const sheets = (
    <>
      {adding ? <DomainSheet open onClose={() => setAdding(false)} onDone={setSelected} /> : null}
      <TrustSheet open={trusting} onClose={() => setTrusting(false)} />
    </>
  );

  if (status.error) {
    const error = toIpcError(status.error);
    return (
      <>
        {toolbar}
        <ErrorState
          title={t("localDomains.loadFailed")}
          message={error.message}
          hint={error.hint}
          action={<Button onClick={() => void status.refetch()}>{t("common.tryAgain")}</Button>}
        />
      </>
    );
  }

  if (status.isSuccess && domains.length === 0) {
    return (
      <>
        {toolbar}
        <EmptyState
          icon={LockKeyhole}
          title={t("localDomains.empty.title")}
          description={t("localDomains.empty.description")}
          action={
            <Button variant="primary" onClick={() => setAdding(true)}>
              {t("localDomains.empty.add")}
            </Button>
          }
        />
        {sheets}
      </>
    );
  }

  return (
    <>
      {toolbar}
      <SplitView
        id="local-domains"
        list={
          status.isPending ? (
            <div className="flex flex-col gap-2 p-3">
              <Skeleton className="h-11" />
              <Skeleton className="h-11" />
            </div>
          ) : (
            <ListPane
              label={t("localDomains.list")}
              items={domains}
              getId={(domain) => domain.name}
              selectedId={current?.name ?? null}
              onSelect={setSelected}
              renderRow={(domain) => {
                const state = domainState(domain, trusted);
                return (
                  <ListRow
                    title={domain.name}
                    subtitle={`→ ${targetText(domain.target, domain.origin)}`}
                    leading={<StatusDot status={state.dot} label={state.label} />}
                    trailing={
                      domain.inspect || !domain.https ? (
                        <span className="flex items-center gap-1.5">
                          {!domain.https ? <Badge>{t("localDomains.http")}</Badge> : null}
                          {domain.inspect ? <Badge>{t("localDomains.inspected")}</Badge> : null}
                        </span>
                      ) : null
                    }
                  />
                );
              }}
            />
          )
        }
      >
        {status.data ? (
          <div className="flex min-h-0 flex-1 flex-col">
            {trusted === false ||
            resolverNeeded ||
            status.data.error ||
            status.data.portProblems.length > 0 ? (
              <div className="flex flex-col gap-2 px-4 pt-3">
                <ServingNotice status={status.data} />
                {trusted === false ? <TrustCallout onSetUp={() => setTrusting(true)} /> : null}
                {resolverNeeded ? <ResolverFix status={status.data} /> : null}
              </div>
            ) : null}
            {current ? (
              <DomainDetail
                key={current.name}
                domain={current}
                status={status.data}
                trusted={trusted}
              />
            ) : null}
          </div>
        ) : (
          <div className="flex flex-col gap-3 p-4">
            <Skeleton className="h-7 w-48" />
            <Skeleton className="h-6" />
            <Skeleton className="h-20" />
          </div>
        )}
      </SplitView>
      {sheets}
    </>
  );
}
