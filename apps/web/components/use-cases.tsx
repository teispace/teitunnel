import {
  Bot,
  Boxes,
  Cable,
  Check,
  Globe,
  HardDrive,
  House,
  Lightbulb,
  LockKeyhole,
  RotateCcw,
  ShieldCheck,
  Thermometer,
  Webhook,
  Zap,
} from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { GuideLink } from "./guide-link";
import type { PickerItem } from "./picker";

/**
 * Each use case as its own small scene, played once when it's picked: parts marked
 * `tt-at` appear at `--at` milliseconds, `tt-until` parts leave then, `tt-between` parts
 * show from `--at` to `--to`, and `tt-swap` stacks a before and an after in one place.
 * Without motion each scene shows how it ends.
 */
const at = (ms: number, to?: number) =>
  ({ "--at": ms, ...(to === undefined ? {} : { "--to": to }) }) as CSSProperties;

function Window({
  title,
  children,
  className = "",
}: {
  title: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`tt-scene-window ${className}`}>
      <div className="tt-scene-bar">
        <span className="flex gap-1" aria-hidden>
          <span className="size-2 rounded-full bg-fd-border" />
          <span className="size-2 rounded-full bg-fd-border" />
          <span className="size-2 rounded-full bg-fd-border" />
        </span>
        <span className="min-w-0 flex-1 truncate text-center font-mono">{title}</span>
      </div>
      {children}
    </div>
  );
}

function Phone({ address, children }: { address: string; children: ReactNode }) {
  return (
    <div className="tt-scene-phone">
      <span className="tt-scene-phone-url">
        <LockKeyhole className="size-2.5 shrink-0" aria-hidden />
        <span className="truncate">{address}</span>
      </span>
      <div className="tt-scene-phone-screen">{children}</div>
    </div>
  );
}

/** A page in miniature: a heading, two lines of text and a button. */
function Page({ button = "var(--tt-accent-fill)" }: { button?: string }) {
  return (
    <div className="flex flex-col gap-2 p-3" aria-hidden>
      <span className="h-2.5 w-3/5 rounded-full bg-fd-foreground/70" />
      <span className="h-1.5 w-4/5 rounded-full bg-fd-muted-foreground/30" />
      <span className="h-1.5 w-2/3 rounded-full bg-fd-muted-foreground/30" />
      <span className="mt-1 h-5 w-16 rounded-md" style={{ background: button }} />
    </div>
  );
}

const violet = "oklch(0.6 0.2 300)";

/** The page on the laptop gets a link, and the same page opens on a phone. */
function Localhost() {
  return (
    <div className="tt-scene-row">
      <Window title="localhost:3000" className="flex-[1.6]">
        <Page />
        <div className="tt-swap px-3 pb-3">
          <span className="tt-until tt-scene-chip" style={at(900)}>
            Share localhost:3000
          </span>
          <span className="tt-at tt-scene-chip tt-scene-chip-live" style={at(900)}>
            <span className="tt-live-dot size-1.5 shrink-0 rounded-full bg-[var(--tt-live)]" />
            <span className="truncate">quiet-river-lamp-orbit.trycloudflare.com</span>
          </span>
        </div>
      </Window>
      <span className="tt-scene-link tt-at" style={at(1200)} aria-hidden>
        <span className="tt-scene-packet" />
      </span>
      <Phone address="quiet-river-lamp-orbit…">
        <div className="tt-at" style={at(1700)}>
          <Page />
        </div>
      </Phone>
    </div>
  );
}

/** A change saved in the editor shows up on the phone at once. */
function DevServer() {
  return (
    <div className="tt-scene-row">
      <Window title="src/Buy.tsx" className="flex-[1.6]">
        <pre className="p-3 font-mono text-[11.5px] leading-relaxed text-fd-muted-foreground">
          <span className="block">export function Buy() {"{"}</span>
          <span className="block"> return (</span>
          <span className="flex">
            {'   <Button color="'}
            <span className="tt-swap">
              <span className="tt-until text-fd-foreground" style={at(1300)}>
                blue
              </span>
              <span className="tt-at text-[var(--tt-accent-text)]" style={at(1300)}>
                violet
              </span>
            </span>
            {'">'}
          </span>
          <span className="block"> );</span>
          <span className="block">{"}"}</span>
        </pre>
        <span
          className="tt-at mx-3 mb-3 inline-flex items-center gap-1 text-[11px] text-fd-muted-foreground"
          style={at(1300)}
        >
          <Check className="size-3" aria-hidden /> Saved
        </span>
      </Window>
      <Phone address="shop.yourhost.com">
        <div className="tt-swap">
          <div className="tt-until" style={at(1900)}>
            <Page />
          </div>
          <div className="tt-at" style={at(1900)}>
            <Page button={violet} />
          </div>
        </div>
        <span className="tt-at tt-scene-toast" style={at(1900)}>
          <Zap className="size-3" aria-hidden /> hot update
        </span>
      </Phone>
    </div>
  );
}

const events: [name: string, status: number, ms: number][] = [
  ["checkout.session.completed", 200, 300],
  ["invoice.paid", 200, 1000],
  ["customer.subscription.updated", 500, 1700],
];

/** Stripe's events arrive signed; one fails, and a replay after the fix goes through. */
function Webhooks() {
  return (
    <Window title="hooks.yourhost.com → localhost:4242">
      <ol className="flex flex-col gap-1.5 p-3 font-mono text-[11.5px]">
        {events.map(([name, status, ms]) => (
          <li key={name} className="tt-at tt-scene-event" style={at(ms)}>
            <span className="text-fd-muted-foreground">POST</span>
            <span className="min-w-0 flex-1 truncate">{name}</span>
            <span className={status < 300 ? "text-[var(--tt-live-text)]" : "text-red-500"}>
              {status}
            </span>
            {status < 300 ? (
              <ShieldCheck className="size-3.5 text-[var(--tt-live-text)]" aria-hidden />
            ) : (
              <span className="tt-at tt-scene-replay" style={at(2600)}>
                <RotateCcw className="size-3" aria-hidden /> Replay
              </span>
            )}
          </li>
        ))}
        <li className="tt-at tt-scene-event tt-scene-event-fixed" style={at(3300)}>
          <RotateCcw className="size-3 text-fd-muted-foreground" aria-hidden />
          <span className="min-w-0 flex-1 truncate">customer.subscription.updated</span>
          <span className="text-[var(--tt-live-text)]">200</span>
          <ShieldCheck className="size-3.5 text-[var(--tt-live-text)]" aria-hidden />
        </li>
      </ol>
      <p className="tt-at px-3 pb-3 text-[11px] text-fd-muted-foreground" style={at(3300)}>
        The handler fixed, the failed event replayed with a fresh signature.
      </p>
    </Window>
  );
}

/** A login code first, then the house, and a light goes on. */
function HomeAssistant() {
  return (
    <div className="tt-scene-row justify-center">
      <Phone address="home.yourhost.com">
        <div className="tt-swap">
          <div className="tt-until flex flex-col gap-2 p-3 text-[10.5px]" style={at(1800)}>
            <span className="font-medium">Get a login code</span>
            <span className="truncate rounded-md border border-fd-border px-2 py-1 text-fd-muted-foreground">
              you@yourhost.com
            </span>
            <span className="flex gap-1" aria-hidden>
              {[3, 8, 1, 6, 0, 4].map((digit, index) => (
                <span
                  // biome-ignore lint/suspicious/noArrayIndexKey: a fixed code
                  key={index}
                  className="tt-at flex size-4 items-center justify-center rounded border border-fd-border font-mono"
                  style={at(300 + index * 180)}
                >
                  {digit}
                </span>
              ))}
            </span>
            <span className="rounded-md bg-[var(--tt-accent-fill)] py-1 text-center text-white">
              Sign in
            </span>
          </div>
          <div className="tt-at flex flex-col gap-1.5 p-2.5 text-[10.5px]" style={at(1800)}>
            <span className="font-medium">Home</span>
            <span className="tt-scene-tile">
              <Lightbulb className="tt-scene-bulb size-3.5" aria-hidden style={at(2500)} />
              Living room
              <span className="tt-scene-toggle" style={at(2500)} aria-hidden />
            </span>
            <span className="tt-scene-tile">
              <LockKeyhole className="size-3.5" aria-hidden /> Front door
              <span className="ms-auto text-fd-muted-foreground">Locked</span>
            </span>
            <span className="tt-scene-tile">
              <Thermometer className="size-3.5" aria-hidden /> Inside
              <span className="ms-auto text-fd-muted-foreground">21.5°</span>
            </span>
          </div>
        </div>
      </Phone>
      <ol className="tt-scene-notes">
        <li className="tt-at" style={at(400)}>
          Cloudflare asks who you are
        </li>
        <li className="tt-at" style={at(1800)}>
          Home Assistant opens, from localhost:8123
        </li>
        <li className="tt-at" style={at(2500)}>
          The router stays closed
        </li>
      </ol>
    </div>
  );
}

const ports: [port: number, ms: number][] = [
  [22, 300],
  [80, 700],
  [443, 1100],
  [8080, 1500],
];

/** Every port of the home connection closed, and the site online anyway. */
function SelfHost() {
  return (
    <div className="tt-scene-row">
      <Window title="port scan of your home IP" className="flex-1">
        <ol className="flex flex-col gap-1.5 p-3 font-mono text-[11.5px]">
          {ports.map(([port, ms]) => (
            <li key={port} className="tt-at flex justify-between" style={at(ms)}>
              <span>:{port}</span>
              <span className="text-fd-muted-foreground">closed</span>
            </li>
          ))}
          <li
            className="tt-at mt-1 border-t border-fd-border pt-2 text-fd-muted-foreground"
            style={at(1900)}
          >
            Nothing to reach from outside.
          </li>
        </ol>
      </Window>
      <Window title="yourhost.com" className="flex-1">
        <Page />
        <div className="tt-at flex flex-col gap-1 px-3 pb-3 text-[11px]" style={at(2400)}>
          <span className="inline-flex items-center gap-1.5 text-[var(--tt-live-text)]">
            <span className="tt-live-dot size-1.5 rounded-full bg-[var(--tt-live)]" /> Online
          </span>
          <span className="text-fd-muted-foreground">Visitors see Cloudflare, not your IP.</span>
        </div>
      </Window>
    </div>
  );
}

/** ssh opens a browser login, and the shell follows. */
function Ssh() {
  return (
    <div className="relative">
      <Window title="zsh">
        <div className="flex min-h-[12.5rem] flex-col gap-1 p-3 font-mono text-[11.5px]">
          <p>
            <span className="text-fd-muted-foreground">$</span> ssh ssh.yourhost.com
          </p>
          <p className="tt-at text-fd-muted-foreground" style={at(600)}>
            A browser window opened to sign in…
          </p>
          <p className="tt-at" style={at(2600)}>
            Welcome to Ubuntu 26.04 LTS
          </p>
          <p className="tt-at" style={at(2900)}>
            <span className="text-[var(--tt-live-text)]">you@server</span>:~${" "}
            <span className="tt-caret inline-block h-3 w-1.5 translate-y-0.5 bg-fd-muted-foreground/70" />
          </p>
        </div>
      </Window>
      <div className="tt-scene-popover tt-between" style={at(900, 2400)}>
        <p className="text-[11px] text-fd-muted-foreground">Cloudflare Access</p>
        <p className="mt-1 font-medium">Sign in to ssh.yourhost.com</p>
        <div className="tt-swap mt-3">
          <span className="tt-until tt-scene-chip justify-center" style={at(1700)}>
            Approve
          </span>
          <span className="tt-at tt-scene-chip tt-scene-chip-live justify-center" style={at(1700)}>
            <Check className="size-3" aria-hidden /> Signed in
          </span>
        </div>
      </div>
    </div>
  );
}

const containers: { name: string; detail: string; route?: string; ms: number }[] = [
  { name: "teitunnel", detail: "the tunnel", ms: 300 },
  { name: "web", detail: "web:80", route: "app.yourhost.com", ms: 700 },
  { name: "api", detail: "api:8080", route: "api.yourhost.com", ms: 1100 },
  { name: "db", detail: "db:5432", ms: 1500 },
];

/** The stack comes up healthy; two containers get hostnames, the database doesn't. */
function Docker() {
  return (
    <Window title="shop · docker compose">
      <div className="grid grid-cols-2 gap-2 p-3 sm:grid-cols-4">
        {containers.map(({ name, detail, route, ms }) => (
          <div
            key={name}
            className="tt-scene-container"
            data-tunnel={name === "teitunnel" ? "" : undefined}
          >
            <span className="flex items-center justify-between gap-1">
              <span className="font-medium">{name}</span>
              <span className="tt-scene-health" style={at(ms)} aria-hidden />
            </span>
            <span className="font-mono text-[10.5px] text-fd-muted-foreground">{detail}</span>
            {route ? (
              <span
                className="tt-at mt-2 truncate font-mono text-[10.5px] text-[var(--tt-accent-text)]"
                style={at(ms + 1000)}
              >
                ← {route}
              </span>
            ) : name === "db" ? (
              <span
                className="tt-at mt-2 inline-flex items-center gap-1 text-[10.5px] text-fd-muted-foreground"
                style={at(2500)}
              >
                <LockKeyhole className="size-3" aria-hidden /> no route
              </span>
            ) : null}
          </div>
        ))}
      </div>
      <p className="tt-at px-3 pb-3 text-[11px] text-fd-muted-foreground" style={at(2500)}>
        Hostnames go to the containers next to it. The database stays private.
      </p>
    </Window>
  );
}

const mcpTools = ["search_notes", "add_task", "list_projects", "summarize"];

/** claude.ai adds the server, you allow the sign-in, and its tools are there. */
function Mcp() {
  return (
    <Window title="claude.ai · Connectors">
      <div className="tt-swap p-4 text-[12px]">
        <div className="tt-until flex flex-col gap-2" style={at(1300)}>
          <span className="font-medium">Add custom connector</span>
          <span className="rounded-md border border-fd-border px-2 py-1.5 font-mono text-[11px]">
            <span className="tt-scene-type">https://mcp.yourhost.com/mcp</span>
          </span>
          <span className="self-end rounded-md bg-fd-foreground px-3 py-1 text-fd-background">
            Add
          </span>
        </div>
        <div className="tt-between flex flex-col gap-2" style={at(1300, 2700)}>
          <span className="text-[11px] text-fd-muted-foreground">mcp.yourhost.com</span>
          <span className="font-medium">claude.ai wants to use your MCP server</span>
          <span className="flex justify-end gap-2">
            <span className="rounded-md border border-fd-border px-3 py-1">Deny</span>
            <span className="rounded-md bg-[var(--tt-accent-fill)] px-3 py-1 text-white">
              Allow
            </span>
          </span>
        </div>
        <div className="tt-at flex flex-col gap-2" style={at(2700)}>
          <span className="inline-flex items-center gap-1.5 font-medium">
            <Check className="size-3.5 text-[var(--tt-live-text)]" aria-hidden /> Connected · 4
            tools
          </span>
          <span className="flex flex-wrap gap-1.5 font-mono text-[10.5px]">
            {mcpTools.map((tool, index) => (
              <span
                key={tool}
                className="tt-at rounded border border-fd-border px-1.5 py-0.5"
                style={at(2900 + index * 150)}
              >
                {tool}
              </span>
            ))}
          </span>
        </div>
      </div>
    </Window>
  );
}

interface Case {
  id: string;
  icon: ReactNode;
  name: string;
  note: string;
  scene: ReactNode;
  facts: string[];
  href: string;
}

const cases: Case[] = [
  {
    id: "localhost",
    icon: <Globe />,
    name: "Expose localhost",
    note: "a link in seconds, no account",
    scene: <Localhost />,
    facts: ["No port forwarding", "HTTPS for free", "Stops when you stop it"],
    href: "/docs/tutorials/expose-localhost/",
  },
  {
    id: "dev-servers",
    icon: <Zap />,
    name: "Share a dev server",
    note: "Vite, Next.js, Django, Rails",
    scene: <DevServer />,
    facts: ["Hot reload works", "The “Blocked request” fix, shown", "Your own domain"],
    href: "/docs/tutorials/dev-servers/",
  },
  {
    id: "webhooks",
    icon: <Webhook />,
    name: "Receive webhooks",
    note: "Stripe, GitHub, Slack",
    scene: <Webhooks />,
    facts: ["A stable address", "Signatures checked", "Replay any delivery"],
    href: "/docs/tutorials/webhooks/",
  },
  {
    id: "home-assistant",
    icon: <House />,
    name: "Home Assistant",
    note: "your home, from anywhere",
    scene: <HomeAssistant />,
    facts: ["No static IP", "A login in front", "Trusted proxies explained"],
    href: "/docs/tutorials/home-assistant/",
  },
  {
    id: "self-host",
    icon: <HardDrive />,
    name: "Self-host from home",
    note: "even behind CGNAT",
    scene: <SelfHost />,
    facts: ["Works behind CGNAT", "Home IP hidden", "Stays up after a restart"],
    href: "/docs/tutorials/self-host/",
  },
  {
    id: "ssh",
    icon: <Cable />,
    name: "SSH from anywhere",
    note: "SSH, RDP, databases",
    scene: <Ssh />,
    facts: ["No inbound port open", "A login in front", "RDP and databases too"],
    href: "/docs/tutorials/ssh/",
  },
  {
    id: "docker",
    icon: <Boxes />,
    name: "Docker Compose",
    note: "one more service",
    scene: <Docker />,
    facts: ["One service added", "Token in a Docker secret", "Health checked"],
    href: "/docs/tutorials/docker-compose/",
  },
  {
    id: "mcp",
    icon: <Bot />,
    name: "An MCP server online",
    note: "for claude.ai and ChatGPT",
    scene: <Mcp />,
    facts: ["Bearer token or OAuth", "Sign-ins you approve", "Requests in the inspector"],
    href: "/docs/tutorials/mcp-server/",
  },
];

export const useCases: PickerItem[] = cases.map((c) => ({
  id: c.id,
  icon: c.icon,
  name: c.name,
  note: c.note,
  panel: (
    <>
      <figure className="tt-scene" aria-label={`${c.name}: ${c.facts.join(", ")}`}>
        {c.scene}
        <ul className="mt-5 flex flex-wrap gap-2">
          {c.facts.map((fact) => (
            <li key={fact} className="tt-case-fact">
              {fact}
            </li>
          ))}
        </ul>
      </figure>
      <GuideLink href={c.href}>Read the guide</GuideLink>
    </>
  ),
}));
