import type { ComponentProps } from "react";
import { cn } from "@/lib/cn";
import { fieldClasses } from "./input";

export function TextArea({ className, rows = 3, ...props }: ComponentProps<"textarea">) {
  return (
    <textarea
      rows={rows}
      spellCheck={false}
      className={cn(fieldClasses, "resize-none py-1", className)}
      {...props}
    />
  );
}
