import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Select } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { TextArea } from "@/components/ui/text-area";
import { HostnameInput } from "@/features/routes";
import { t } from "@/lib/i18n";
import type { ZoneRef } from "@/lib/ipc/bindings";

export type Protection = "none" | "password" | "login";
export type Expiry = "never" | "1" | "7" | "30";

export interface DetailsState {
  name: string;
  address: "domain" | "workersDev";
  hostname: string;
  protection: Protection;
  password: string;
  allowed: string;
  spa: boolean;
  expires: Expiry;
}

interface DetailsStepProps {
  state: DetailsState;
  onChange: (next: DetailsState) => void;
  zones: readonly ZoneRef[];
  /** Update mode: name and address are fixed, and an empty password keeps the current one. */
  updating: boolean;
  hasPassword: boolean;
  fieldError: (field: string) => string | null;
}

/** Name, address, who can open it, and how long it stays. */
export function DetailsStep({
  state,
  onChange,
  zones,
  updating,
  hasPassword,
  fieldError,
}: DetailsStepProps) {
  const set = (patch: Partial<DetailsState>) => onChange({ ...state, ...patch });
  return (
    <div className="flex flex-col gap-4">
      {updating ? null : (
        <>
          <Field
            label={t("snapshots.details.name")}
            help={t("snapshots.details.nameHelp")}
            error={fieldError("name")}
          >
            {(control) => (
              <Input
                {...control}
                autoComplete="off"
                value={state.name}
                onChange={(event) => set({ name: event.target.value })}
              />
            )}
          </Field>
          <div className="flex flex-col gap-2">
            <SegmentedControl
              label={t("snapshots.details.address")}
              segments={[
                { value: "domain", label: t("snapshots.details.myDomain") },
                { value: "workersDev", label: t("snapshots.details.workersDev") },
              ]}
              value={zones.length > 0 ? state.address : "workersDev"}
              onValueChange={(address) => set({ address })}
              className="self-start"
            />
            {state.address === "domain" && zones.length > 0 ? (
              <Field label={t("snapshots.details.hostname")} error={fieldError("hostname")}>
                {(control) => (
                  <HostnameInput
                    zones={zones}
                    value={state.hostname}
                    onChange={(hostname) => set({ hostname })}
                    id={control.id}
                    describedBy={control["aria-describedby"]}
                    invalid={control["aria-invalid"] === true}
                  />
                )}
              </Field>
            ) : (
              <p className="text-callout text-secondary">{t("snapshots.details.workersDevHelp")}</p>
            )}
          </div>
        </>
      )}
      <Field label={t("snapshots.details.protection")}>
        {(control) => (
          <Select
            id={control.id}
            label={t("snapshots.details.protection")}
            options={[
              { value: "none", label: t("snapshots.details.protectionNone") },
              { value: "password", label: t("snapshots.details.protectionPassword") },
              ...(state.address === "domain" && zones.length > 0
                ? [{ value: "login" as const, label: t("snapshots.details.protectionLogin") }]
                : []),
            ]}
            value={state.protection}
            onValueChange={(protection) => set({ protection })}
          />
        )}
      </Field>
      {state.protection === "password" ? (
        <Field
          label={t("snapshots.details.password")}
          help={
            updating && hasPassword
              ? t("snapshots.details.passwordKeep")
              : t("snapshots.details.passwordHelp")
          }
          error={fieldError("password")}
        >
          {(control) => (
            <Input
              {...control}
              type="password"
              autoComplete="new-password"
              value={state.password}
              onChange={(event) => set({ password: event.target.value })}
            />
          )}
        </Field>
      ) : null}
      {state.protection === "login" ? (
        <Field
          label={t("snapshots.details.allowed")}
          help={t("snapshots.details.allowedHelp")}
          error={fieldError("access")}
        >
          {(control) => (
            <TextArea
              {...control}
              rows={2}
              spellCheck={false}
              value={state.allowed}
              onChange={(event) => set({ allowed: event.target.value })}
            />
          )}
        </Field>
      ) : null}
      {state.protection === "password" ? (
        <p className="text-footnote text-secondary">{t("snapshots.details.passwordLimit")}</p>
      ) : null}
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="text-body">{t("snapshots.details.spa")}</p>
          <p className="text-callout text-secondary">{t("snapshots.details.spaHelp")}</p>
        </div>
        <Switch
          aria-label={t("snapshots.details.spa")}
          checked={state.spa}
          onCheckedChange={(spa) => set({ spa })}
        />
      </div>
      <Field label={t("snapshots.details.expires")}>
        {(control) => (
          <Select
            id={control.id}
            label={t("snapshots.details.expires")}
            options={[
              { value: "never", label: t("snapshots.details.expiresNever") },
              { value: "1", label: t("snapshots.details.expiresDays", { count: 1 }) },
              { value: "7", label: t("snapshots.details.expiresDays", { count: 7 }) },
              { value: "30", label: t("snapshots.details.expiresDays", { count: 30 }) },
            ]}
            value={state.expires}
            onValueChange={(expires) => set({ expires })}
          />
        )}
      </Field>
    </div>
  );
}
