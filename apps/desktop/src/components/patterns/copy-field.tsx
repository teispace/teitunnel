import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";

interface CopyFieldProps {
  value: string;
  label: string;
  className?: string;
}

const CONFIRM_MS = 1400;

/** A selectable monospace value with a copy button that confirms with a tick. */
export function CopyField({ value, label, className }: CopyFieldProps) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), CONFIRM_MS);
  };

  return (
    <div
      className={cn(
        "flex h-6 min-w-0 items-center rounded-control bg-surface-inset pr-0.5 pl-2",
        className,
      )}
    >
      <span className="selectable min-w-0 flex-1 truncate font-mono text-mono" title={value}>
        {value}
      </span>
      <button
        type="button"
        aria-label={copied ? `${label} copied` : `Copy ${label}`}
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
        {copied ? "Copied" : ""}
      </span>
    </div>
  );
}
