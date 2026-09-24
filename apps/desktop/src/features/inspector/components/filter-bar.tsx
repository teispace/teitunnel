import { Input } from "@/components/ui/input";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { Select } from "@/components/ui/select";
import { t } from "@/lib/i18n";
import { DURATIONS, type Filters, formatMs, METHODS, type StatusFilter } from "../model";

const statuses = (): { value: StatusFilter; label: string }[] => [
  { value: "all", label: t("inspector.filter.statusAll") },
  { value: "2", label: "2xx" },
  { value: "3", label: "3xx" },
  { value: "4", label: "4xx" },
  { value: "5", label: "5xx" },
  { value: "errors", label: t("inspector.filter.errors") },
];

interface FilterBarProps {
  search: string;
  onSearch: (search: string) => void;
  filters: Filters;
  onFilters: (filters: Filters) => void;
}

/**
 * Search (headers and bodies, in the inspector) and the list's filters: status class,
 * method, path or host text, duration.
 */
export function FilterBar({ search, onSearch, filters, onFilters }: FilterBarProps) {
  return (
    <div className="flex flex-wrap items-center gap-2 border-separator border-b-hairline px-3 py-2">
      <Input
        type="search"
        aria-label={t("inspector.filter.search")}
        placeholder={t("inspector.filter.searchPlaceholder")}
        value={search}
        onChange={(event) => onSearch(event.target.value)}
        className="min-w-24 flex-1 rounded-full"
      />
      <Input
        aria-label={t("inspector.filter.text")}
        placeholder={t("inspector.filter.textPlaceholder")}
        value={filters.text}
        onChange={(event) => onFilters({ ...filters, text: event.target.value })}
        className="w-24 shrink-0 font-mono text-mono"
      />
      <SegmentedControl
        label={t("inspector.filter.status")}
        size="sm"
        segments={statuses()}
        value={filters.status}
        onValueChange={(status) => onFilters({ ...filters, status })}
      />
      <Select
        label={t("inspector.filter.method")}
        options={[
          { value: "any", label: t("inspector.filter.anyMethod") },
          ...METHODS.map((method) => ({ value: method, label: method })),
        ]}
        value={filters.method || "any"}
        onValueChange={(method) =>
          onFilters({ ...filters, method: method === "any" ? "" : method })
        }
        className="w-[7.5rem]"
      />
      <Select
        label={t("inspector.filter.duration")}
        options={DURATIONS.map((ms) => ({
          value: String(ms),
          label:
            ms === 0
              ? t("inspector.filter.anyDuration")
              : t("inspector.filter.slower", { duration: formatMs(ms) }),
        }))}
        value={String(filters.minDurationMs)}
        onValueChange={(ms) => onFilters({ ...filters, minDurationMs: Number(ms) })}
        className="w-[8.5rem]"
      />
    </div>
  );
}
