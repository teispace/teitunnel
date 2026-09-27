import { ArrowRight } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";

/** A link to a guide, with an arrow that nudges on hover. */
export function GuideLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link
      href={href}
      className="group mt-4 inline-flex items-center gap-1 self-start text-sm font-medium underline-offset-4 hover:underline"
    >
      {children}
      <ArrowRight
        className="size-3.5 transition-transform group-hover:translate-x-0.5"
        aria-hidden
      />
    </Link>
  );
}
