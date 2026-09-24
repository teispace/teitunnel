import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, type ReactNode, useState } from "react";
import { CopyField } from "@/components/patterns/copy-field";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { TextArea } from "@/components/ui/text-area";
import { t } from "@/lib/i18n";
import type { AgentPreset, ProtectionInput, TapView } from "@/lib/ipc/bindings";
import { toIpcError } from "@/lib/ipc/client";
import { useProtectTap } from "../queries";

const PRESETS: readonly AgentPreset[] = ["aiCrawlers", "searchEngines", "seoCrawlers"];

const lines = (text: string) =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

function Row({ title, status, children }: { title: string; status: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2 border-separator border-b-hairline pb-4 last:border-b-0">
      <div className="flex items-center gap-2">
        <h3 className="text-headline">{title}</h3>
        <span className="text-callout text-secondary">{status}</span>
      </div>
      {children}
    </section>
  );
}

/**
 * Protection enforced by the inspector on this computer: a password page, a secret link,
 * basic auth, bearer tokens, IP lists, user agents and paths that skip it. Changes apply
 * at once; a new link key or token is shown here once and never again.
 */
export function TapProtection({ tap }: { tap: TapView }) {
  const protect = useProtectTap();
  const navigate = useNavigate();
  const current = tap.protection;
  const [password, setPassword] = useState("");
  const [user, setUser] = useState(current.basicUser ?? "");
  const [basicPassword, setBasicPassword] = useState("");
  const [linkKey, setLinkKey] = useState<string | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [allow, setAllow] = useState(current.ipAllow.join("\n"));
  const [deny, setDeny] = useState(current.ipDeny.join("\n"));
  const [presets, setPresets] = useState<AgentPreset[]>(current.agentPresets);
  const [patterns, setPatterns] = useState(current.agentPatterns.join("\n"));
  const [bypass, setBypass] = useState(current.bypass.join("\n"));
  const [busy, setBusy] = useState<string | null>(null);

  const run = (what: string, input: ProtectionInput, after?: () => void) => {
    setBusy(what);
    protect.mutate(
      { tap: tap.id, input },
      {
        onSuccess: (result) => {
          if (result.secretLinkKey) setLinkKey(result.secretLinkKey);
          if (result.bearerToken) setToken(result.bearerToken);
          after?.();
        },
        onSettled: () => setBusy(null),
      },
    );
  };
  const pending = (what: string) => busy === what && protect.isPending;
  const on = t("inspector.protection.on");
  const off = t("inspector.protection.off");
  const route = tap.scope.kind === "route" ? tap.scope : null;
  const linkUrl =
    linkKey && tap.publicUrl
      ? `${tap.publicUrl.replace(/\/$/, "")}/?key=${encodeURIComponent(linkKey)}`
      : linkKey;

  const submitPassword = (event: FormEvent) => {
    event.preventDefault();
    if (password) run("password", { password }, () => setPassword(""));
  };
  const submitBasic = (event: FormEvent) => {
    event.preventDefault();
    if (user.trim() && basicPassword) {
      run("basic", { basic: [user.trim(), basicPassword] }, () => setBasicPassword(""));
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1 rounded-row bg-surface-pressed px-3 py-2.5 text-callout">
        <p>{t("inspector.protection.where")}</p>
        {route ? (
          <p className="flex flex-wrap items-center gap-2 text-secondary">
            {t("inspector.protection.edge")}
            <Button
              size="sm"
              onClick={() => void navigate({ to: "/routes", search: { route: route.hostname } })}
            >
              {t("inspector.protection.openRoute")}
            </Button>
          </p>
        ) : (
          <p className="text-secondary">{t("inspector.protection.quickShare")}</p>
        )}
      </div>

      <Row title={t("inspector.protection.password")} status={current.password ? on : off}>
        <form onSubmit={submitPassword} className="flex items-center gap-2">
          <Input
            type="password"
            autoComplete="new-password"
            aria-label={t("inspector.protection.passwordField")}
            placeholder={t("inspector.protection.passwordField")}
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            className="min-w-0 flex-1"
          />
          <Button size="sm" type="submit" disabled={!password} pending={pending("password")}>
            {t("inspector.protection.setPassword")}
          </Button>
          {current.password ? (
            <Button
              size="sm"
              variant="destructive"
              pending={pending("passwordOff")}
              onClick={() => run("passwordOff", { password: "" })}
            >
              {t("inspector.protection.remove")}
            </Button>
          ) : null}
        </form>
      </Row>

      <Row title={t("inspector.protection.link")} status={current.secretLink ? on : off}>
        <div className="flex items-center gap-2">
          <p className="min-w-0 flex-1 text-callout text-secondary">
            {t("inspector.protection.linkHelp")}
          </p>
          <Button
            size="sm"
            pending={pending("link")}
            onClick={() => run("link", { secretLink: true })}
          >
            {current.secretLink
              ? t("inspector.protection.newLink")
              : t("inspector.protection.createLink")}
          </Button>
          {current.secretLink ? (
            <Button
              size="sm"
              variant="destructive"
              pending={pending("linkOff")}
              onClick={() => run("linkOff", { secretLink: false }, () => setLinkKey(null))}
            >
              {t("inspector.protection.remove")}
            </Button>
          ) : null}
        </div>
        {linkUrl ? (
          <ShownOnce>
            <CopyField label={t("inspector.protection.link")} value={linkUrl} />
          </ShownOnce>
        ) : null}
      </Row>

      <Row
        title={t("inspector.protection.basic")}
        status={
          current.basicUser ? t("inspector.protection.basicUser", { user: current.basicUser }) : off
        }
      >
        <form onSubmit={submitBasic} className="flex items-center gap-2">
          <Input
            autoComplete="off"
            aria-label={t("inspector.protection.user")}
            placeholder={t("inspector.protection.user")}
            value={user}
            onChange={(event) => setUser(event.target.value)}
            className="w-32"
          />
          <Input
            type="password"
            autoComplete="new-password"
            aria-label={t("inspector.protection.basicPassword")}
            placeholder={t("inspector.protection.basicPassword")}
            value={basicPassword}
            onChange={(event) => setBasicPassword(event.target.value)}
            className="min-w-0 flex-1"
          />
          <Button
            size="sm"
            type="submit"
            disabled={!user.trim() || !basicPassword}
            pending={pending("basic")}
          >
            {t("inspector.protection.set")}
          </Button>
          {current.basicUser ? (
            <Button
              size="sm"
              variant="destructive"
              pending={pending("basicOff")}
              onClick={() => run("basicOff", { basic: ["", ""] })}
            >
              {t("inspector.protection.remove")}
            </Button>
          ) : null}
        </form>
      </Row>

      <Row
        title={t("inspector.protection.bearer")}
        status={
          current.bearerTokens > 0
            ? t("inspector.protection.tokens", { count: current.bearerTokens })
            : off
        }
      >
        <div className="flex items-center gap-2">
          <p className="min-w-0 flex-1 text-callout text-secondary">
            {t("inspector.protection.bearerHelp")}
          </p>
          <Button
            size="sm"
            pending={pending("bearer")}
            onClick={() => run("bearer", { bearer: true })}
          >
            {t("inspector.protection.createToken")}
          </Button>
          {current.bearerTokens > 0 ? (
            <Button
              size="sm"
              variant="destructive"
              pending={pending("bearerOff")}
              onClick={() => run("bearerOff", { bearer: false }, () => setToken(null))}
            >
              {t("inspector.protection.removeTokens")}
            </Button>
          ) : null}
        </div>
        {token ? (
          <ShownOnce>
            <CopyField label={t("inspector.protection.bearer")} value={token} />
          </ShownOnce>
        ) : null}
      </Row>

      <section className="flex flex-col gap-3">
        <div className="grid grid-cols-2 gap-3">
          <ListField
            id="protect-allow"
            label={t("inspector.protection.allow")}
            value={allow}
            onChange={setAllow}
            placeholder="203.0.113.0/24"
          />
          <ListField
            id="protect-deny"
            label={t("inspector.protection.deny")}
            value={deny}
            onChange={setDeny}
            placeholder="198.51.100.7"
          />
        </div>
        <p className="-mt-2 text-callout text-secondary">{t("inspector.protection.ipHelp")}</p>
        <fieldset className="flex flex-col gap-1.5">
          <legend className="mb-1 text-body">{t("inspector.protection.agents")}</legend>
          {PRESETS.map((preset) => (
            <label
              key={preset}
              htmlFor={`protect-${preset}`}
              className="flex items-center gap-2 text-body"
            >
              <Checkbox
                id={`protect-${preset}`}
                checked={presets.includes(preset)}
                onCheckedChange={(checked) =>
                  setPresets(
                    checked === true ? [...presets, preset] : presets.filter((p) => p !== preset),
                  )
                }
              />
              {t(`inspector.protection.preset.${preset}`)}
            </label>
          ))}
        </fieldset>
        <ListField
          id="protect-patterns"
          label={t("inspector.protection.patterns")}
          value={patterns}
          onChange={setPatterns}
          placeholder="curl/"
        />
        <ListField
          id="protect-bypass"
          label={t("inspector.protection.bypass")}
          value={bypass}
          onChange={setBypass}
          placeholder="/webhooks/*"
        />
        <p className="-mt-2 text-callout text-secondary">{t("inspector.protection.bypassHelp")}</p>
        <div>
          <Button
            size="sm"
            pending={pending("lists")}
            onClick={() =>
              run("lists", {
                ipAllow: lines(allow),
                ipDeny: lines(deny),
                agentPresets: presets,
                agentPatterns: lines(patterns),
                bypass: lines(bypass),
              })
            }
          >
            {t("inspector.protection.saveLists")}
          </Button>
        </div>
      </section>
      {protect.error ? (
        <p role="alert" className="text-callout text-error">
          {toIpcError(protect.error).message}
        </p>
      ) : null}
    </div>
  );
}

function ShownOnce({ children }: { children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      {children}
      <p role="status" className="text-callout text-warning">
        {t("inspector.protection.shownOnce")}
      </p>
    </div>
  );
}

function ListField({
  id,
  label,
  value,
  onChange,
  placeholder,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-body">
        {label}
      </label>
      <TextArea
        id={id}
        rows={3}
        className="font-mono text-mono"
        placeholder={placeholder}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}
