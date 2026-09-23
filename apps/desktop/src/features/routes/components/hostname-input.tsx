import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { t } from "@/lib/i18n";
import type { ZoneRef } from "@/lib/ipc/bindings";

/** Splits `app.example.com` into the part before the longest matching zone and the zone. */
export function splitHostname(hostname: string, zones: readonly ZoneRef[]) {
  const host = hostname.trim().toLowerCase();
  const zone = [...zones]
    .sort((a, b) => b.name.length - a.name.length)
    .find((z) => host === z.name || host.endsWith(`.${z.name}`));
  if (!zone) return { sub: host, zone: zones[0]?.name ?? "" };
  return { sub: host === zone.name ? "" : host.slice(0, -zone.name.length - 1), zone: zone.name };
}

export function joinHostname(sub: string, zone: string) {
  const name = sub.trim().replace(/\.+$/, "");
  return name ? `${name}.${zone}` : zone;
}

interface HostnameInputProps {
  zones: readonly ZoneRef[];
  value: string;
  onChange: (hostname: string) => void;
  id?: string;
  describedBy?: string | undefined;
  invalid?: boolean;
  autoFocus?: boolean;
}

/**
 * A subdomain field joined to a domain pop-up: `app` · `example.com`. Empty subdomain
 * means the domain itself.
 */
export function HostnameInput({
  zones,
  value,
  onChange,
  id,
  describedBy,
  invalid,
  autoFocus,
}: HostnameInputProps) {
  const { sub, zone } = splitHostname(value, zones);
  return (
    <div className="flex min-w-0 items-center gap-1.5">
      <Input
        {...(id ? { id } : {})}
        aria-label={t("hostname.subdomain")}
        aria-describedby={describedBy}
        aria-invalid={invalid || undefined}
        autoFocus={autoFocus}
        autoComplete="off"
        spellCheck={false}
        placeholder="app"
        value={sub}
        onChange={(event) => onChange(joinHostname(event.target.value, zone))}
        className="min-w-0 flex-1 text-right font-mono text-mono"
      />
      <span aria-hidden className="text-body text-secondary">
        .
      </span>
      <Select
        label={t("hostname.domain")}
        options={zones.map((z) => ({ value: z.name, label: z.name }))}
        value={zone}
        onValueChange={(next) => onChange(joinHostname(sub, next))}
        className="max-w-[55%]"
      />
    </div>
  );
}
