import { ArrowRight } from "lucide-react";
import Link from "next/link";
import { Fragment, type ReactNode } from "react";
import { Points } from "./points";

/**
 * One job in the feature tour, as a card in a stack: on wide screens each card stays near
 * the top while the next one slides over it, and the covered card sinks back. The promise
 * and its points on one side, the app doing it (with a live demo floating over it) on the
 * other.
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
    <article id={id} data-scene="stack" className="tt-card scroll-mt-[3.25rem]">
      <div className="tt-card-inner grid grid-cols-1 gap-10 p-6 sm:p-8 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-12 lg:p-12">
        <div className="flex min-w-0 flex-col">
          <p className="mb-3 font-mono text-xs uppercase tracking-[0.18em] text-[var(--tt-accent-text)]">
            {eyebrow}
          </p>
          <h3 className="text-2xl font-semibold tracking-tight text-balance md:text-[2.1rem] md:leading-tight">
            {title}
          </h3>
          <p className="mt-4 text-fd-muted-foreground md:text-lg">{lead}</p>
          <div className="mt-7">
            <Points
              items={bullets.map((bullet, index) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: a fixed list
                <Fragment key={index}>{bullet}</Fragment>
              ))}
            />
          </div>
          <div className="mt-6 flex flex-wrap gap-x-5 gap-y-2 lg:mt-auto lg:pt-6">
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
        <div data-scene="view" className="tt-card-media min-w-0 self-center">
          {media}
        </div>
      </div>
    </article>
  );
}
