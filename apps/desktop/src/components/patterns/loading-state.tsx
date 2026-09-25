import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";

/**
 * Says what's being waited for, once a wait gets long enough to wonder: it fades in after
 * `delay` ms (CSS only), so quick loads never show it.
 */
export function SlowHint({
  children,
  delay = 1500,
  className,
}: {
  children: string;
  delay?: number;
  className?: string;
}) {
  return (
    <p
      aria-live="polite"
      className={cn(
        "flex items-center gap-2 text-callout text-secondary opacity-0 [animation:fade-in_200ms_ease-out_forwards]",
        className,
      )}
      style={{ animationDelay: `${delay}ms` }}
    >
      <Spinner className="size-3.5" label={null} />
      {children}
    </p>
  );
}

/**
 * A screen that isn't ready yet: a spinner after 300 ms (DESIGN §1), and what it's doing
 * if it takes longer. The router shows it while a screen's code loads.
 */
export function LoadingState({ label }: { label?: string }) {
  return (
    <div
      role="status"
      aria-busy
      aria-label={label ?? t("common.loading")}
      className="flex h-full items-center justify-center pb-(--toolbar-height)"
    >
      <div className="flex flex-col items-center gap-3 opacity-0 [animation:fade-in_200ms_ease-out_300ms_forwards]">
        <Spinner className="size-5" label={null} />
        {label ? <p className="text-callout text-secondary">{label}</p> : null}
      </div>
    </div>
  );
}
