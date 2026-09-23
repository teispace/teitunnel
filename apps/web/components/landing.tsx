import Link from "next/link";
import type { ReactNode } from "react";
import { asset } from "@/lib/site";

/** A screenshot of the app, in the reader's color scheme. */
export function Shot({ name, alt, priority }: { name: string; alt: string; priority?: boolean }) {
  const loading = priority ? "eager" : "lazy";
  return (
    <div className="overflow-hidden rounded-xl border border-fd-border bg-fd-card shadow-2xl shadow-black/10 dark:shadow-black/40">
      {/* biome-ignore lint/performance/noImgElement: static export, pre-sized screenshots */}
      <img
        src={asset(`/screens/${name}-light.png`)}
        alt={alt}
        width={2560}
        height={1600}
        loading={loading}
        className="block h-auto w-full dark:hidden"
      />
      {/* biome-ignore lint/performance/noImgElement: static export, pre-sized screenshots */}
      <img
        src={asset(`/screens/${name}-dark.png`)}
        alt={alt}
        width={2560}
        height={1600}
        loading={loading}
        className="hidden h-auto w-full dark:block"
      />
    </div>
  );
}

export function Section({
  id,
  eyebrow,
  title,
  children,
  className = "",
}: {
  id?: string;
  eyebrow?: string;
  title: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section id={id} className={`mx-auto w-full max-w-6xl px-6 py-20 md:py-28 ${className}`}>
      <div className="mb-12 max-w-3xl">
        {eyebrow ? (
          <p className="mb-3 font-mono text-xs uppercase tracking-[0.18em] text-fd-muted-foreground">
            {eyebrow}
          </p>
        ) : null}
        <h2 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">{title}</h2>
      </div>
      {children}
    </section>
  );
}

export function ButtonLink({
  href,
  children,
  primary = false,
  external = false,
}: {
  href: string;
  children: ReactNode;
  primary?: boolean;
  external?: boolean;
}) {
  const className = primary
    ? "inline-flex h-11 items-center gap-2 rounded-full bg-fd-foreground px-6 text-sm font-medium text-fd-background transition-opacity hover:opacity-85"
    : "inline-flex h-11 items-center gap-2 rounded-full border border-fd-border px-6 text-sm font-medium transition-colors hover:bg-fd-accent";
  if (external) {
    return (
      <a href={href} className={className} target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    );
  }
  return (
    <Link href={href} className={className}>
      {children}
    </Link>
  );
}
