import { Check, Minus } from "lucide-react";
import { Checkbox as CheckboxPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";

export function Checkbox({ className, ...props }: ComponentProps<typeof CheckboxPrimitive.Root>) {
  return (
    <CheckboxPrimitive.Root
      className={cn(
        "peer inline-flex size-3.5 shrink-0 items-center justify-center rounded-[4px] outline-offset-1",
        "border-hairline border-control bg-surface-toggle shadow-control",
        "data-[state=checked]:border-transparent data-[state=checked]:bg-accent data-[state=checked]:text-on-accent",
        "data-[state=indeterminate]:border-transparent data-[state=indeterminate]:bg-accent data-[state=indeterminate]:text-on-accent",
        "disabled:opacity-40",
        className,
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator className="flex items-center justify-center">
        {props.checked === "indeterminate" ? (
          <Minus aria-hidden className="size-2.5" strokeWidth={3} />
        ) : (
          <Check aria-hidden className="size-2.5" strokeWidth={3} />
        )}
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  );
}
