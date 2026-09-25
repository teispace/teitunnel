import { CircleAlert, CircleCheck, CircleDashed } from "lucide-react";
import { useId, useState } from "react";
import { ConfirmDialog } from "@/components/patterns/confirm-dialog";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { t } from "@/lib/i18n";
import type { TrustState, TrustStoreView, TrustView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useRunAsAdmin, useTrust, useTrustCa, useUntrustCa } from "../queries";

function stateText(state: TrustState): string {
  switch (state.state) {
    case "toolMissing":
      return state.package
        ? t("localDomains.trust.state.toolMissingPackage", {
            tool: state.tool,
            package: state.package,
          })
        : t("localDomains.trust.state.toolMissing", { tool: state.tool });
    case "error":
      return t("localDomains.trust.state.error", { message: state.message });
    default:
      return t(`localDomains.trust.state.${state.state}`);
  }
}

function StoreRow({ store }: { store: TrustStoreView }) {
  const good = store.state.state === "trusted" || store.state.state === "followsSystem";
  const bad = store.state.state === "error" || store.state.state === "toolMissing";
  const Icon = good ? CircleCheck : bad ? CircleAlert : CircleDashed;
  return (
    <li className="flex items-start gap-2 py-1.5">
      <Icon
        aria-hidden
        className={`mt-0.5 size-4 shrink-0 ${good ? "text-healthy" : bad ? "text-warning" : "text-tertiary"}`}
      />
      <div className="min-w-0 flex-1">
        <div className="text-body">{t(`localDomains.store.${store.kind}`)}</div>
        <div className="text-callout text-secondary">{stateText(store.state)}</div>
        {store.path ? (
          <div className="selectable truncate font-mono text-footnote text-tertiary">
            {store.path}
          </div>
        ) : null}
      </div>
    </li>
  );
}

interface TrustSheetProps {
  open: boolean;
  onClose: () => void;
}

/**
 * The trust walkthrough: what trusting the local certificate authority means, the
 * button that does it (the system asks for a password or a confirmation), every store
 * and its state, and the steps left that need an administrator, with a way to run them
 * on Linux. It checks again every few seconds until the system trusts it.
 */
export function TrustSheet({ open, onClose }: TrustSheetProps) {
  const [browsers, setBrowsers] = useState(true);
  const [firefox, setFirefox] = useState(false);
  const [result, setResult] = useState<TrustView | null>(null);
  const trust = useTrust({ enabled: open, recheck: open && result !== null });
  const view = trust.data ?? result;
  const trustCa = useTrustCa();
  const untrust = useUntrustCa();
  const admin = useRunAsAdmin();
  const ids = useId();
  const platform = view?.platform ?? "macos";
  const trusted = view?.trusted === true;
  const steps = result?.steps ?? [];
  const error = trustCa.error ?? admin.error;

  const close = () => {
    setResult(null);
    trustCa.reset();
    admin.reset();
    onClose();
  };
  const run = () =>
    trustCa.mutate(
      { browsers: platform === "linux" || browsers, firefoxSystemRoots: firefox },
      { onSuccess: setResult },
    );

  return (
    <Sheet open={open} onOpenChange={(next) => (next ? undefined : close())}>
      <SheetContent
        title={t("localDomains.trust.title")}
        description={t("localDomains.trust.description")}
        footer={
          trusted ? (
            <Button variant="primary" onClick={close}>
              {t("localDomains.trust.done")}
            </Button>
          ) : (
            <>
              <Button variant="plain" onClick={close}>
                {t("common.cancel")}
              </Button>
              <Button variant="primary" onClick={run} pending={trustCa.isPending}>
                {t("localDomains.trust.trust")}
              </Button>
            </>
          )
        }
      >
        <div className="flex flex-col gap-4">
          <ol className="flex list-decimal flex-col gap-1.5 pl-5 text-body">
            <li>{t("localDomains.trust.step1")}</li>
            <li>{t("localDomains.trust.step2")}</li>
            <li>{t("localDomains.trust.step3")}</li>
          </ol>
          <p className="text-callout text-secondary">{t("localDomains.trust.safety")}</p>
          {!trusted && platform !== "linux" ? (
            <div className="flex flex-col gap-2">
              <label htmlFor={`${ids}-browsers`} className="flex items-start gap-2">
                <Checkbox
                  id={`${ids}-browsers`}
                  className="mt-0.5"
                  checked={browsers}
                  onCheckedChange={(value) => setBrowsers(value === true)}
                />
                <span className="text-body">{t("localDomains.trust.browsers")}</span>
              </label>
              <label htmlFor={`${ids}-firefox`} className="flex items-start gap-2">
                <Checkbox
                  id={`${ids}-firefox`}
                  className="mt-0.5"
                  checked={firefox}
                  onCheckedChange={(value) => setFirefox(value === true)}
                />
                <span className="flex flex-col">
                  <span className="text-body">{t("localDomains.trust.firefox")}</span>
                  <span className="text-callout text-secondary">
                    {t("localDomains.trust.firefoxDetail")}
                  </span>
                </span>
              </label>
            </div>
          ) : null}
          {view && view.stores.length > 0 ? (
            <section aria-label={t("localDomains.trust.stores")}>
              <h3 className="text-headline">{t("localDomains.trust.stores")}</h3>
              <ul className="mt-1 divide-y-(length:--hairline) divide-inset">
                {view.stores.map((store) => (
                  <StoreRow key={`${store.kind}:${store.path ?? ""}`} store={store} />
                ))}
              </ul>
            </section>
          ) : null}
          {steps.length > 0 ? (
            <section className="flex flex-col gap-2" aria-label={t("localDomains.trust.admin")}>
              <h3 className="text-headline">{t("localDomains.trust.admin")}</h3>
              <p className="text-callout text-secondary">{t("localDomains.trust.adminDetail")}</p>
              <CopyField
                multiline
                label={t("localDomains.trust.copyCommands")}
                value={steps.map((s) => s.command).join("\n")}
              />
              {platform === "linux" ? (
                <div>
                  <Button onClick={() => admin.mutate("trustStore")} pending={admin.isPending}>
                    {t("localDomains.runAsAdmin")}
                  </Button>
                </div>
              ) : null}
            </section>
          ) : null}
          {trusted ? (
            <p className="flex items-center gap-2 text-callout text-healthy" role="status">
              <CircleCheck aria-hidden className="size-4" /> {t("localDomains.trust.trusted")}
            </p>
          ) : result ? (
            <p className="text-callout text-secondary" role="status">
              {t("localDomains.trust.checking")}
            </p>
          ) : null}
          {error ? (
            <p role="alert" className="text-callout text-error">
              {toIpcError(error).message}
            </p>
          ) : null}
          {trusted ? (
            <div>
              <ConfirmDialog
                trigger={<Button variant="plain">{t("localDomains.trust.stop")}</Button>}
                title={t("localDomains.trust.stopTitle")}
                description={t("localDomains.trust.stopDetail")}
                confirmLabel={t("localDomains.trust.stopConfirm")}
                variant="destructive"
                onConfirm={() => untrust.mutateAsync(false)}
              />
            </div>
          ) : null}
        </div>
      </SheetContent>
    </Sheet>
  );
}
