import { ArrowRight, Check } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";
import { delay } from "./landing";

/**
 * One job in the feature tour: the promise and what it means above, the app doing it below
 * at a size where its text can be read.
 */
export function Feature({
  id,
  eyebrow,
  title,
  lead,
  bullets,
  links,
  media,
}: {
  id?: string;
  eyebrow: string;
  title: ReactNode;
  lead: ReactNode;
  bullets: ReactNode[];
  links: [label: string, href: string][];
  media: ReactNode;
}) {
  return (
    <article id={id} className="scroll-mt-24">
      <div className="grid grid-cols-1 gap-8 lg:grid-cols-2 lg:gap-16">
        <div data-reveal className="min-w-0">
          <p className="mb-3 font-mono text-xs uppercase tracking-[0.18em] text-[var(--tt-accent-text)]">
            {eyebrow}
          </p>
          <h3 className="text-2xl font-semibold tracking-tight text-balance md:text-4xl">
            {title}
          </h3>
          <p className="mt-4 text-fd-muted-foreground md:text-lg">{lead}</p>
        </div>
        <div className="min-w-0 lg:pt-8">
          <ul className="flex flex-col gap-3">
            {bullets.map((bullet, index) => (
              <li
                // biome-ignore lint/suspicious/noArrayIndexKey: a fixed list
                key={index}
                data-reveal
                style={delay(60 + index * 60)}
                className="flex gap-3 text-[15px]"
              >
                <Check className="mt-1 size-4 shrink-0 text-[var(--tt-accent-text)]" aria-hidden />
                <span className="text-fd-muted-foreground [&_strong]:font-medium [&_strong]:text-fd-foreground">
                  {bullet}
                </span>
              </li>
            ))}
          </ul>
          <div data-reveal style={delay(300)} className="mt-6 flex flex-wrap gap-x-5 gap-y-2">
            {links.map(([label, href]) => (
              <Link
                key={href}
                href={href}
                className="group inline-flex items-center gap-1 text-sm font-medium underline-offset-4 hover:underline"
              >
                {label}
                <ArrowRight
                  className="size-3.5 transition-transform group-hover:translate-x-0.5"
                  aria-hidden
                />
              </Link>
            ))}
          </div>
        </div>
      </div>
      <div data-reveal style={delay(120)} className="mt-10 min-w-0 md:mt-14">
        {media}
      </div>
    </article>
  );
}
