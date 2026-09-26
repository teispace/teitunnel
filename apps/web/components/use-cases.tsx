import {
  Bot,
  Boxes,
  Cable,
  Globe,
  HardDrive,
  House,
  Laptop,
  LockKeyhole,
  type LucideIcon,
  Server,
  ShieldCheck,
  Smartphone,
  Users,
  Webhook,
  Zap,
} from "lucide-react";
import { type CSSProperties, Fragment } from "react";
import { GuideLink } from "./guide-link";
import type { PickerItem } from "./picker";

interface Case {
  id: string;
  icon: LucideIcon;
  name: string;
  note: string;
  /** Who sends the requests. */
  from: { icon: LucideIcon; label: string; detail: string };
  /** The public address, and what Cloudflare checks there, if anything. */
  address: string;
  guard?: string;
  /** Where the requests end up. */
  to: { icon: LucideIcon; label: string; detail: string };
  facts: string[];
  href: string;
}

const cases: Case[] = [
  {
    id: "localhost",
    icon: Globe,
    name: "Expose localhost",
    note: "a link in seconds, no account",
    from: { icon: Users, label: "A client", detail: "or your phone" },
    address: "quiet-river-lamp-orbit.trycloudflare.com",
    to: { icon: Laptop, label: "Your laptop", detail: "localhost:3000" },
    facts: ["No port forwarding", "HTTPS for free", "Stops when you stop it"],
    href: "/docs/tutorials/expose-localhost/",
  },
  {
    id: "dev-servers",
    icon: Zap,
    name: "Share a dev server",
    note: "Vite, Next.js, Django, Rails",
    from: { icon: Smartphone, label: "Your phone", detail: "hot reload included" },
    address: "shop.yourhost.com",
    to: { icon: Laptop, label: "Vite", detail: "localhost:5173" },
    facts: ["The “Blocked request” fix, shown", "Hot reload works", "Your own domain"],
    href: "/docs/tutorials/dev-servers/",
  },
  {
    id: "webhooks",
    icon: Webhook,
    name: "Receive webhooks",
    note: "Stripe, GitHub, Slack",
    from: { icon: Webhook, label: "Stripe", detail: "checkout.session.completed" },
    address: "hooks.yourhost.com",
    to: { icon: Laptop, label: "Your API", detail: "localhost:4242" },
    facts: ["A stable address", "Signatures checked", "Replay any delivery"],
    href: "/docs/tutorials/webhooks/",
  },
  {
    id: "home-assistant",
    icon: House,
    name: "Home Assistant",
    note: "your home, from anywhere",
    from: { icon: Smartphone, label: "Your phone", detail: "away from home" },
    address: "home.yourhost.com",
    guard: "Login",
    to: { icon: House, label: "Home Assistant", detail: "localhost:8123" },
    facts: ["Router left closed", "No static IP", "Trusted proxies explained"],
    href: "/docs/tutorials/home-assistant/",
  },
  {
    id: "self-host",
    icon: HardDrive,
    name: "Self-host from home",
    note: "even behind CGNAT",
    from: { icon: Users, label: "Visitors", detail: "anywhere" },
    address: "yourhost.com",
    to: { icon: HardDrive, label: "Home server", detail: "nas.local:80" },
    facts: ["Works behind CGNAT", "Home IP hidden", "Stays up after a restart"],
    href: "/docs/tutorials/self-host/",
  },
  {
    id: "ssh",
    icon: Cable,
    name: "SSH from anywhere",
    note: "SSH, RDP, databases",
    from: { icon: Laptop, label: "Your laptop", detail: "ssh via cloudflared" },
    address: "ssh.yourhost.com",
    guard: "Login",
    to: { icon: Server, label: "A server", detail: "localhost:22" },
    facts: ["No inbound port open", "A login in front", "RDP and databases too"],
    href: "/docs/tutorials/ssh/",
  },
  {
    id: "docker",
    icon: Boxes,
    name: "Docker Compose",
    note: "one more service",
    from: { icon: Users, label: "Visitors", detail: "anywhere" },
    address: "app.yourhost.com",
    to: { icon: Boxes, label: "The web container", detail: "web:80" },
    facts: ["One service added", "Token in a Docker secret", "Health checked"],
    href: "/docs/tutorials/docker-compose/",
  },
  {
    id: "mcp",
    icon: Bot,
    name: "An MCP server online",
    note: "for claude.ai and ChatGPT",
    from: { icon: Bot, label: "claude.ai", detail: "or ChatGPT" },
    address: "mcp.yourhost.com",
    guard: "OAuth",
    to: { icon: Laptop, label: "Your MCP server", detail: "localhost:8000" },
    facts: ["Bearer token or OAuth", "Sign-ins you approve", "Requests in the inspector"],
    href: "/docs/tutorials/mcp-server/",
  },
];

function Node({
  icon: Icon,
  label,
  detail,
  className = "",
}: {
  icon: LucideIcon;
  label: string;
  detail: string;
  className?: string;
}) {
  return (
    <span className={`tt-case-node ${className}`}>
      <span className="tt-case-node-icon" aria-hidden>
        <Icon className="size-4" />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-sm font-medium">{label}</span>
        <span className="block truncate font-mono text-[11px] text-fd-muted-foreground">
          {detail}
        </span>
      </span>
    </span>
  );
}

/**
 * One use case as the path its requests take: from whoever sends them, to the public
 * address at Cloudflare (with its login, if there is one), down the tunnel your computer
 * opened, to the service. Requests travel it while it's shown.
 */
function Path({ c }: { c: Case }) {
  return (
    <figure
      className="tt-case"
      aria-label={`${c.from.label} reaches ${c.to.label} at ${c.address} through Cloudflare`}
    >
      <div className="tt-case-map">
        <Node {...c.from} />
        <span className="tt-case-wire" aria-hidden>
          <span className="tt-case-packet" style={{ "--d": "0s" } as CSSProperties} />
          <span className="tt-case-packet" style={{ "--d": "1.1s" } as CSSProperties} />
        </span>
        <span className="tt-case-edge">
          <span className="font-mono text-[11px] tracking-wide text-fd-muted-foreground uppercase">
            Cloudflare
          </span>
          <span className="mt-1 block font-mono text-[13px] text-[var(--tt-accent-text)]">
            {/* Long addresses wrap at their dots. */}
            {c.address.split(".").map((part, index) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: parts of a fixed address
              <Fragment key={index}>
                {index > 0 ? (
                  <>
                    .<wbr />
                  </>
                ) : null}
                {part}
              </Fragment>
            ))}
          </span>
          <span className="mt-2 inline-flex items-center gap-1 text-[11px] text-fd-muted-foreground">
            {c.guard ? (
              <>
                <LockKeyhole className="size-3 text-[var(--tt-accent-text)]" aria-hidden />
                {c.guard} first
              </>
            ) : (
              <>
                <ShieldCheck className="size-3" aria-hidden /> HTTPS
              </>
            )}
          </span>
        </span>
        <span className="tt-case-wire tt-case-tunnel" aria-hidden>
          <span className="tt-case-packet" style={{ "--d": "0.55s" } as CSSProperties} />
          <span className="tt-case-packet" style={{ "--d": "1.65s" } as CSSProperties} />
          <span className="tt-case-tunnel-label">tunnel · outbound only</span>
        </span>
        <Node {...c.to} className="tt-case-to" />
      </div>
      <ul className="mt-7 flex flex-wrap justify-center gap-2">
        {c.facts.map((fact, index) => (
          <li
            key={fact}
            className="tt-case-fact tt-pick-result"
            style={{ "--tt-delay": index * 90 } as CSSProperties}
          >
            {fact}
          </li>
        ))}
      </ul>
    </figure>
  );
}

export const useCases: PickerItem[] = cases.map((c) => ({
  id: c.id,
  icon: <c.icon />,
  name: c.name,
  note: c.note,
  panel: (
    <>
      <Path c={c} />
      <GuideLink href={c.href}>Read the guide</GuideLink>
    </>
  ),
}));
