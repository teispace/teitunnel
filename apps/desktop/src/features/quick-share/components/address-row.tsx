import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { t } from "@/lib/i18n";
import { useExpandedName, useNameSuggestions } from "../queries";

/** A random trycloudflare.com address, or a subdomain of one of the account's domains. */
export const RANDOM = "random";

/**
 * Where a share goes: a random address, or a name on one of the account's domains, with
 * names suggested from the project (`{project}`, `{branch}`) and a preview of what a name
 * with placeholders becomes.
 */
export function AddressRow({
  domains,
  address,
  onAddressChange,
  subdomain,
  onSubdomainChange,
  folder,
  project,
}: {
  domains: readonly string[];
  address: string;
  onAddressChange: (address: string) => void;
  subdomain: string;
  onSubdomainChange: (subdomain: string) => void;
  /** The project folder of what's shared (for `{branch}`, and the name used there last). */
  folder: string | null;
  /** The project's name, when only that is known. */
  project: string | null;
}) {
  const onDomain = address !== RANDOM;
  const suggestions = useNameSuggestions(onDomain ? address : null, folder, project).data ?? [];
  const typed = subdomain.trim().replace(/\.$/, "");
  const hostname = typed ? `${typed}.${address}` : address;
  const expanded = useExpandedName(onDomain ? hostname : "", folder);
  const suffix = `.${address}`;
  const labels = suggestions
    .filter((s) => s.template.endsWith(suffix))
    .map((s) => ({ ...s, label: s.template.slice(0, -suffix.length) }))
    .filter((s) => s.label !== typed)
    .slice(0, 3);

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <Select
          label={t("quickShare.address.label")}
          options={[
            { value: RANDOM, label: t("quickShare.address.random") },
            ...domains.map((name) => ({
              value: name,
              label: t("quickShare.address.onDomain", { domain: name }),
            })),
          ]}
          value={address}
          onValueChange={onAddressChange}
          className="h-7"
        />
        {onDomain ? (
          <div className="flex min-w-0 flex-1 items-center gap-1">
            <Input
              aria-label={t("quickShare.address.subdomain")}
              placeholder={t("quickShare.address.subdomainPlaceholder")}
              autoComplete="off"
              spellCheck={false}
              className="h-7 min-w-0 flex-1 text-right font-mono text-mono"
              value={subdomain}
              onChange={(event) => onSubdomainChange(event.target.value)}
            />
            <span className="shrink-0 font-mono text-mono text-secondary">.{address}</span>
          </div>
        ) : null}
      </div>
      {onDomain && typed.includes("{") ? (
        <p className="px-1 text-footnote text-secondary" aria-live="polite">
          {expanded.data
            ? t("quickShare.names.becomes", { hostname: expanded.data })
            : expanded.error
              ? t("quickShare.names.invalid")
              : " "}
        </p>
      ) : null}
      {onDomain && labels.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1 px-1">
          <span className="text-footnote text-secondary">{t("quickShare.names.suggested")}</span>
          {labels.map((s) => (
            <button
              key={s.template}
              type="button"
              title={s.hostname}
              onClick={() => onSubdomainChange(s.label)}
              className="rounded-full bg-surface-control px-2 py-0.5 font-mono text-footnote text-primary outline-offset-1 active:bg-surface-control-pressed"
            >
              {s.remembered
                ? t("quickShare.names.lastTime", { name: s.hostname.slice(0, -suffix.length) })
                : s.hostname.slice(0, -suffix.length)}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
