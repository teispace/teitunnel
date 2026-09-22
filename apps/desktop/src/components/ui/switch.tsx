import { Switch as SwitchPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";

/** macOS switch: accent track when on, white knob sliding on the snappy spring. */
export function Switch({ className, ...props }: ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      className={cn(
        "group relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full outline-offset-1",
        "bg-surface-control-pressed transition-colors transition-snappy",
        "data-[state=checked]:bg-accent disabled:opacity-40",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        className={cn(
          "pointer-events-none block h-3.5 w-3.5 translate-x-0.5 rounded-full bg-surface-knob",
          "shadow-[0_1px_2px_rgb(0_0_0/0.25),0_0_0_0.5px_rgb(0_0_0/0.06)]",
          "transition-[translate,width] transition-snappy data-[state=checked]:translate-x-4",
          "group-active:w-[18px] group-active:data-[state=checked]:translate-x-3",
        )}
      />
    </SwitchPrimitive.Root>
  );
}
