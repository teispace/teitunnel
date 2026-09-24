import { t } from "@/lib/i18n";
import type { HeaderView } from "@/lib/ipc/bindings";

/** Headers in order, names in the left column (selectable, wrapped). */
export function HeadersView({ headers }: { headers: readonly HeaderView[] }) {
  if (headers.length === 0) {
    return <p className="text-callout text-secondary">{t("inspector.detail.noHeaders")}</p>;
  }
  return (
    <dl className="selectable grid grid-cols-[minmax(5rem,max-content)_minmax(0,1fr)] gap-x-3 gap-y-0.5 font-mono text-mono">
      {headers.map((header, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: repeated headers are positional
        <div key={index} className="contents">
          <dt className="max-w-44 truncate text-secondary" title={header.name}>
            {header.name}
          </dt>
          <dd className="min-w-0 break-all">{header.value}</dd>
        </div>
      ))}
    </dl>
  );
}
