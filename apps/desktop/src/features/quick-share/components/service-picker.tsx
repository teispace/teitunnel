import { Popover as PopoverPrimitive } from "radix-ui";
import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";
import type { LocalService, ServiceKind } from "@/lib/ipc/bindings";
import { useLocalServices } from "../queries";

const kindLabels: Record<ServiceKind, string> = {
  vite: "Vite",
  next: "Next.js",
  astro: "Astro",
  nuxt: "Nuxt",
  remix: "Remix",
  django: "Django",
  flask: "Flask",
  fastApi: "FastAPI",
  rails: "Rails",
  laravel: "Laravel",
  hugo: "Hugo",
  jekyll: "Jekyll",
  node: "Node.js",
  python: "Python",
  ruby: "Ruby",
  php: "PHP",
  java: "Java",
  go: "Go",
  docker: "Docker",
  database: "Database",
  system: "macOS",
  other: "",
};

function describe(service: LocalService): string {
  if (service.kind === "system") {
    const name = service.process === "ControlCenter" ? t("quickShare.airplay") : service.process;
    return `${name} · macOS`;
  }
  const kind =
    service.kind === "database"
      ? t("quickShare.kind.database")
      : kindLabels[service.kind] || service.process;
  return service.project ? `${kind} · ${service.project}` : kind;
}

function matches(service: LocalService, query: string): boolean {
  const q = query
    .trim()
    .toLowerCase()
    .replace(/^(https?:\/\/)?(localhost|127\.0\.0\.1)?:?/, "");
  if (!q) return true;
  return (
    String(service.port).startsWith(q) ||
    describe(service).toLowerCase().includes(q) ||
    service.process.toLowerCase().includes(q)
  );
}

interface ServicePickerProps {
  autoFocus?: boolean;
  value: string;
  onChange: (value: string) => void;
  invalid?: boolean;
  describedBy?: string | undefined;
}

/**
 * A text field for a port or address, with a list of services detected on this Mac.
 * Typing filters the list; arrows and Enter pick; Escape closes it.
 */
export function ServicePicker({
  autoFocus = false,
  value,
  onChange,
  invalid,
  describedBy,
}: ServicePickerProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (autoFocus) inputRef.current?.focus();
  }, [autoFocus]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const listId = useId();
  const { data: services = [] } = useLocalServices(open);
  const shareable = services.filter((s) => s.kind !== "database" && matches(s, value));

  const pick = (service: LocalService) => {
    onChange(String(service.port));
    setOpen(false);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setOpen(true);
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setActive((i) => Math.min(Math.max(i + delta, 0), Math.max(shareable.length - 1, 0)));
    } else if (event.key === "Enter" && open && shareable[active]) {
      event.preventDefault();
      pick(shareable[active]);
    } else if (event.key === "Escape") {
      setOpen(false);
    }
  };

  return (
    <PopoverPrimitive.Root open={open && shareable.length > 0} onOpenChange={setOpen}>
      <PopoverPrimitive.Anchor asChild>
        <div className="min-w-0 flex-1">
          <Input
            ref={inputRef}
            role="combobox"
            aria-label={t("quickShare.portOrAddress")}
            aria-expanded={open}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-invalid={invalid || undefined}
            aria-describedby={describedBy}
            {...(open && shareable[active]
              ? { "aria-activedescendant": `${listId}-${active}` }
              : {})}
            placeholder={t("quickShare.placeholder")}
            value={value}
            onChange={(event) => {
              onChange(event.target.value);
              setActive(0);
              setOpen(true);
            }}
            onFocus={() => setOpen(true)}
            onBlur={() => setOpen(false)}
            onKeyDown={onKeyDown}
            className="h-7"
          />
        </div>
      </PopoverPrimitive.Anchor>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="start"
          sideOffset={4}
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
          className="z-50 w-(--radix-popover-trigger-width) min-w-72 rounded-[10px] p-1 material-panel shadow-raised data-[state=open]:animate-fade-in"
        >
          <div className="px-2 pt-1 pb-1.5 text-footnote font-semibold text-tertiary">
            {t("quickShare.running")}
          </div>
          <div
            id={listId}
            role="listbox"
            aria-label={t("quickShare.detected")}
            className="max-h-64 overflow-y-auto"
          >
            {shareable.map((service, index) => (
              <div
                key={service.port}
                id={`${listId}-${index}`}
                role="option"
                aria-selected={index === active}
                tabIndex={-1}
                onMouseDown={(event) => {
                  event.preventDefault();
                  pick(service);
                }}
                onMouseMove={() => setActive(index)}
                className={cn(
                  "flex h-7 items-center gap-2 rounded-[6px] px-2 text-body",
                  index === active && "bg-accent-fill text-on-accent",
                  service.kind === "system" && index !== active && "text-secondary",
                )}
              >
                <span className="min-w-0 flex-1 truncate">{describe(service)}</span>
                <span
                  className={cn(
                    "font-mono text-mono tabular",
                    index !== active && "text-secondary",
                  )}
                >
                  :{service.port}
                </span>
              </div>
            ))}
          </div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
