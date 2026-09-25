import { Globe, Laptop, Network, Users } from "lucide-react";
import type { ComponentType, CSSProperties } from "react";

const nodes: {
  icon: ComponentType<{ className?: string }>;
  title: string;
  detail: string;
}[] = [
  { icon: Laptop, title: "Your service", detail: "localhost:3000" },
  { icon: Network, title: "Teitunnel", detail: "runs cloudflared for you" },
  { icon: Globe, title: "Cloudflare", detail: "your account, your domain" },
  { icon: Users, title: "Visitors", detail: "https://app.teispace.com" },
];

/**
 * How a request travels: four stops joined by lines with requests moving along them
 * (CSS only; still with Reduce Motion). Vertical on phones.
 */
export function TunnelFlow() {
  return (
    <figure
      aria-label="Visitors reach Cloudflare, which sends requests through the tunnel Teitunnel opened from your computer to your service"
      className="rounded-2xl border border-fd-border bg-fd-card p-5 md:p-8"
    >
      <ol className="flex flex-col items-stretch gap-0 md:flex-row md:items-center">
        {nodes.map(({ icon: Icon, title, detail }, index) => (
          <li key={title} className="flex flex-col items-center md:flex-1 md:flex-row">
            <div className="flex w-full items-center gap-3 rounded-xl border border-fd-border bg-fd-background px-4 py-3 md:w-auto md:flex-1 md:flex-col md:items-center md:gap-2 md:px-3 md:py-5 md:text-center">
              <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-fd-border bg-fd-card">
                <Icon className="size-4.5 text-[var(--tt-accent)]" />
              </span>
              <span className="min-w-0">
                <span className="block text-sm font-medium">{title}</span>
                <span className="block truncate font-mono text-xs text-fd-muted-foreground">
                  {detail}
                </span>
              </span>
            </div>
            {index < nodes.length - 1 ? (
              <span
                aria-hidden
                className="tt-wire relative my-1 h-8 w-px bg-fd-border md:mx-1 md:my-0 md:h-px md:w-10 md:shrink-0 lg:w-14"
                style={{ "--tt-delay": index * 400 } as CSSProperties}
              />
            ) : null}
          </li>
        ))}
      </ol>
      <figcaption className="mt-5 text-center text-sm text-fd-muted-foreground">
        The connection starts on your computer, so there's no port to open and your IP address stays
        private.
      </figcaption>
    </figure>
  );
}
