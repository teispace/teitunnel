import { ChevronRight } from "lucide-react";
import { Collapsible } from "radix-ui";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

interface DisclosureProps {
  title: ReactNode;
  children: ReactNode;
  defaultOpen?: boolean;
  className?: string;
}

/** A disclosure triangle section (NSDisclosureButton). */
export function Disclosure({ title, children, defaultOpen = false, className }: DisclosureProps) {
  return (
    <Collapsible.Root defaultOpen={defaultOpen} className={className}>
      <Collapsible.Trigger className="group flex h-6 items-center gap-1 rounded-control pr-2 text-headline text-primary outline-offset-0">
        <ChevronRight
          aria-hidden
          className="size-3.5 text-secondary transition-transform transition-snappy group-data-[state=open]:rotate-90"
          strokeWidth={2.25}
        />
        {title}
      </Collapsible.Trigger>
      <Collapsible.Content
        className={cn(
          "overflow-hidden pl-[18px]",
          "data-[state=closed]:animate-[collapse_var(--duration-smooth)_var(--ease-smooth)]",
          "data-[state=open]:animate-[expand_var(--duration-smooth)_var(--ease-smooth)]",
        )}
      >
        <div className="pt-1">{children}</div>
      </Collapsible.Content>
    </Collapsible.Root>
  );
}
