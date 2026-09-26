import type { ReactNode } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { type MessageKey, t } from "@/lib/i18n";
import type { OriginOptions } from "@/lib/ipc/bindings";

type Flag =
  | "noTLSVerify"
  | "matchSNItoHost"
  | "http2Origin"
  | "disableChunkedEncoding"
  | "noHappyEyeballs";
type Text = "httpHostHeader" | "originServerName" | "caPool";
type Seconds = "connectTimeout" | "tlsTimeout" | "keepAliveTimeout" | "tcpKeepAlive";

/** Whether any setting differs from cloudflared's defaults. */
export function hasOriginSettings(options: OriginOptions): boolean {
  return Object.values(options).some(
    (value) => value !== undefined && value !== null && value !== false && value !== "",
  );
}

/** The settings to send: `null` when all are defaults (nothing to write). */
export function originSettingsToSend(options: OriginOptions): OriginOptions | null {
  return hasOriginSettings(options) ? options : null;
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className="flex flex-col gap-3">
      <legend className="mb-2 text-footnote font-semibold text-secondary">{title}</legend>
      {children}
    </fieldset>
  );
}

/**
 * A route's origin settings (`originRequest`), in three groups. Empty fields and
 * unticked boxes mean cloudflared's defaults.
 */
export function OriginSettings({
  value,
  onChange,
  error,
}: {
  value: OriginOptions;
  onChange: (next: OriginOptions) => void;
  error?: string | undefined;
}) {
  const set = (patch: Partial<OriginOptions>) => onChange({ ...value, ...patch });

  const flag = (key: Flag, label: MessageKey) => (
    <label htmlFor={`origin-${key}`} className="flex items-center gap-2 text-body">
      <Checkbox
        id={`origin-${key}`}
        checked={value[key] === true}
        onCheckedChange={(checked) => set({ [key]: checked === true })}
      />
      {t(label)}
    </label>
  );

  const text = (key: Text, label: MessageKey, help: MessageKey, placeholder: string) => (
    <Field label={t(label)} help={t(help)}>
      {(control) => (
        <Input
          {...control}
          placeholder={placeholder}
          autoComplete="off"
          className="font-mono text-mono"
          value={value[key] ?? ""}
          onChange={(event) => set({ [key]: event.target.value || null })}
        />
      )}
    </Field>
  );

  const number = (
    key: Seconds | "keepAliveConnections",
    label: MessageKey,
    placeholder: string,
  ) => (
    <Field label={t(label)}>
      {(control) => (
        <Input
          {...control}
          type="number"
          inputMode="numeric"
          min={1}
          placeholder={placeholder}
          value={value[key] ?? ""}
          onChange={(event) => {
            const parsed = Number.parseInt(event.target.value, 10);
            set({ [key]: Number.isNaN(parsed) ? null : parsed });
          }}
        />
      )}
    </Field>
  );

  return (
    <div className="flex flex-col gap-5">
      <Group title={t("routeSheet.origin.https")}>
        {flag("noTLSVerify", "routeSheet.origin.noTlsVerify")}
        {text(
          "originServerName",
          "routeSheet.origin.serverName",
          "routeSheet.origin.serverNameHelp",
          "origin.example.com",
        )}
        {flag("matchSNItoHost", "routeSheet.origin.matchSni")}
        {text(
          "caPool",
          "routeSheet.origin.caPool",
          "routeSheet.origin.caPoolHelp",
          "/etc/ssl/ca.pem",
        )}
        {flag("http2Origin", "routeSheet.origin.http2")}
      </Group>
      <Group title={t("routeSheet.origin.requests")}>
        {text(
          "httpHostHeader",
          "routeSheet.origin.hostHeader",
          "routeSheet.origin.hostHeaderHelp",
          "localhost",
        )}
        {flag("disableChunkedEncoding", "routeSheet.origin.noChunked")}
        <Field label={t("routeSheet.origin.proxy")}>
          {(control) => (
            <Select
              id={control.id}
              label={t("routeSheet.origin.proxy")}
              options={[
                { value: "none", label: t("routeSheet.origin.proxyNone") },
                { value: "socks", label: t("routeSheet.origin.proxySocks") },
              ]}
              value={value.proxyType === "socks" ? "socks" : "none"}
              onValueChange={(next) => set({ proxyType: next === "socks" ? "socks" : null })}
            />
          )}
        </Field>
      </Group>
      <Group title={t("routeSheet.origin.connection")}>
        <div className="grid grid-cols-2 gap-3">
          {number("connectTimeout", "routeSheet.origin.connectTimeout", "30")}
          {number("tlsTimeout", "routeSheet.origin.tlsTimeout", "10")}
          {number("keepAliveTimeout", "routeSheet.origin.keepAliveTimeout", "90")}
          {number("tcpKeepAlive", "routeSheet.origin.tcpKeepAlive", "30")}
          {number("keepAliveConnections", "routeSheet.origin.keepAliveConnections", "100")}
        </div>
        {flag("noHappyEyeballs", "routeSheet.origin.noHappyEyeballs")}
      </Group>
      {error ? (
        <p role="alert" className="text-callout text-error">
          {error}
        </p>
      ) : null}
    </div>
  );
}
