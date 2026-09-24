import Link from "next/link";
import type { CSSProperties, ReactNode } from "react";
import { asset, site } from "@/lib/site";
import { Logo } from "./logo";

/** Animation delay (ms) for `data-reveal` / `data-hero` elements. */
export function delay(ms: number): CSSProperties {
  return { "--tt-delay": ms } as CSSProperties;
}

/** A screenshot of the app, in the reader's color scheme (`public/screens/<name>-<scheme>.webp`). */
export function Shot({
  name,
  alt,
  priority,
  width = 2560,
  height = 1600,
  className = "",
}: {
  name: string;
  alt: string;
  priority?: boolean;
  width?: number;
  height?: number;
  className?: string;
}) {
  const loading = priority ? "eager" : "lazy";
  const image = "block h-auto w-full";
  return (
    <div
      className={`overflow-hidden rounded-xl border border-fd-border bg-fd-card shadow-2xl shadow-black/10 dark:shadow-black/40 ${className}`}
    >
      {/* biome-ignore lint/performance/noImgElement: static export, pre-sized screenshots */}
      <img
        src={asset(`/screens/${name}-light.webp`)}
        alt={alt}
        width={width}
        height={height}
        loading={loading}
        decoding="async"
        className={`${image} dark:hidden`}
      />
      {/* biome-ignore lint/performance/noImgElement: static export, pre-sized screenshots */}
      <img
        src={asset(`/screens/${name}-dark.webp`)}
        alt={alt}
        width={width}
        height={height}
        loading={loading}
        decoding="async"
        className={`${image} hidden dark:block`}
      />
    </div>
  );
}

export function Section({
  id,
  eyebrow,
  title,
  lead,
  children,
  className = "",
}: {
  id?: string;
  eyebrow?: string;
  title: ReactNode;
  lead?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section id={id} className={`mx-auto w-full max-w-6xl px-6 py-20 md:py-28 ${className}`}>
      <div data-reveal className="mb-12 max-w-3xl">
        {eyebrow ? (
          <p className="mb-3 font-mono text-xs uppercase tracking-[0.18em] text-[var(--tt-accent)]">
            {eyebrow}
          </p>
        ) : null}
        <h2 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">{title}</h2>
        {lead ? <p className="mt-5 text-lg text-fd-muted-foreground">{lead}</p> : null}
      </div>
      {children}
    </section>
  );
}

/** The site's pill buttons. */
export function buttonClass(primary: boolean): string {
  return primary
    ? "inline-flex h-11 items-center gap-2 rounded-full bg-fd-foreground px-6 text-sm font-medium text-fd-background transition-[opacity,transform] duration-200 hover:opacity-85 active:scale-[0.98]"
    : "inline-flex h-11 items-center gap-2 rounded-full border border-fd-border bg-fd-background/60 px-6 text-sm font-medium backdrop-blur transition-[background-color,transform] duration-200 hover:bg-fd-accent active:scale-[0.98]";
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
  const className = buttonClass(primary);
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

const footerColumns: { title: string; links: [string, string][] }[] = [
  {
    title: "Product",
    links: [
      ["Download", "/download/"],
      ["Features", "/#features"],
      ["Use cases", "/#use-cases"],
      ["Command line", "/docs/reference/cli/"],
      ["Release notes", site.releases],
    ],
  },
  {
    title: "Learn",
    links: [
      ["What is Cloudflare Tunnel?", "/docs/concepts/cloudflare-tunnel/"],
      ["Expose localhost", "/docs/tutorials/expose-localhost/"],
      ["Your first route", "/docs/getting-started/first-route/"],
      ["Servers and Docker", "/docs/guides/servers/"],
      ["Troubleshooting", "/docs/reference/troubleshooting/"],
    ],
  },
  {
    title: "Project",
    links: [
      ["GitHub", site.github],
      ["Report an issue", site.issues],
      ["Security", "/docs/reference/security/"],
      ["Privacy", "/privacy/"],
    ],
  },
];

function FooterLink({ href, children }: { href: string; children: ReactNode }) {
  const className = "transition-colors hover:text-fd-foreground";
  return href.startsWith("http") ? (
    <a href={href} target="_blank" rel="noopener noreferrer" className={className}>
      {children}
    </a>
  ) : (
    <Link href={href} className={className}>
      {children}
    </Link>
  );
}

/** The footer of the landing and download pages. */
export function SiteFooter() {
  return (
    <footer className="border-t border-fd-border bg-fd-card/40">
      <div className="mx-auto grid w-full max-w-6xl gap-12 px-6 py-16 text-sm md:grid-cols-[1.4fr_repeat(3,1fr)]">
        <div className="flex max-w-xs flex-col gap-4">
          <Link href="/" className="flex items-center gap-2 text-base font-semibold">
            <Logo className="size-5" />
            {site.name}
          </Link>
          <p className="text-fd-muted-foreground">
            Crafted by{" "}
            <a
              href={site.org.url}
              target="_blank"
              rel="noopener noreferrer"
              className="font-medium text-fd-foreground underline-offset-4 hover:underline"
            >
              {site.org.name}
            </a>{" "}
            in Nepal, and open source for everyone who ships from localhost.
          </p>
        </div>
        {footerColumns.map((column) => (
          <nav key={column.title} aria-label={column.title} className="flex flex-col gap-3">
            <p className="font-medium">{column.title}</p>
            <ul className="flex flex-col gap-2.5 text-fd-muted-foreground">
              {column.links.map(([label, href]) => (
                <li key={label}>
                  <FooterLink href={href}>{label}</FooterLink>
                </li>
              ))}
            </ul>
          </nav>
        ))}
      </div>
      <div className="border-t border-fd-border">
        <div className="mx-auto flex w-full max-w-6xl flex-col gap-3 px-6 py-6 text-xs text-fd-muted-foreground md:flex-row md:items-center md:justify-between">
          <p>
            © {new Date().getFullYear()} {site.org.legalName}. MIT licensed.
          </p>
          <p className="max-w-xl md:text-right">
            Cloudflare and cloudflared are trademarks of Cloudflare, Inc. Teitunnel is an
            independent project and isn't endorsed by Cloudflare.
          </p>
        </div>
      </div>
    </footer>
  );
}
