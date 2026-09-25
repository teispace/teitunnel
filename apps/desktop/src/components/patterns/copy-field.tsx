import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { t } from "@/lib/i18n";

interface CopyFieldProps {
  value: string;
  label: string;
  className?: string;
  /** Show every line (e.g. a config snippet) instead of one truncated line. */
  multiline?: boolean;
  /**
   * Copies instead of the shown value (e.g. a secret copied from Rust while the field
   * shows a mask, so it never reaches the webview).
   */
  onCopy?: () => Promise<unknown>;
}

const CONFIRM_MS = 1400;

/** A selectable monospace value with a copy button that confirms with a tick. */
export function CopyField({ value, label, className, multiline = false, onCopy }: CopyFieldProps) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = async () => {
    if (onCopy) await onCopy();
    else await navigator.clipboard.writeText(value);
    setCopied(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), CONFIRM_MS);
  };

  return (
    <div
      className={cn(
        "flex min-w-0 rounded-control bg-surface-inset pr-0.5 pl-2",
        multiline ? "items-start py-0.5" : "h-6 items-center",
        className,
      )}
    >
      <span
        className={cn(
          "selectable min-w-0 flex-1 font-mono text-mono",
          multiline ? "overflow-x-auto whitespace-pre py-0.5" : "truncate",
        )}
        title={multiline ? undefined : value}
      >
        {value}
      </span>
      <button
        type="button"
        aria-label={copied ? t("copy.copiedLabel", { label }) : t("copy.copy", { label })}
        onClick={() => void copy()}
        className="relative flex size-5 shrink-0 items-center justify-center rounded-full text-secondary outline-offset-0 active:bg-surface-control"
      >
        <Copy
          aria-hidden
          className={cn(
            "absolute size-3.5 transition-[opacity,scale] transition-snappy",
            copied && "scale-50 opacity-0",
          )}
          strokeWidth={1.75}
        />
        <Check
          aria-hidden
          className={cn(
            "absolute size-3.5 text-healthy transition-[opacity,scale] duration-(--duration-bouncy) ease-(--ease-bouncy)",
            !copied && "scale-50 opacity-0",
          )}
          strokeWidth={2.25}
        />
      </button>
      <span aria-live="polite" className="sr-only">
        {copied ? t("copy.copied") : ""}
      </span>
    </div>
  );
}
