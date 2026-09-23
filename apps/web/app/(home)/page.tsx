import {
  Activity,
  ArrowRight,
  Boxes,
  Download,
  GitBranch,
  Globe,
  KeyRound,
  LockKeyhole,
  Network,
  Scale,
  ServerCog,
  ShieldCheck,
  Stethoscope,
  Terminal,
  Undo2,
  Zap,
} from "lucide-react";
import type { ReactNode } from "react";
import { ButtonLink, Section, Shot } from "@/components/landing";
import { site } from "@/lib/site";

const capabilities: { icon: typeof Zap; title: string; body: string; href: string }[] = [
  {
    icon: Globe,
    title: "Your own domains",
    body: "Route app.example.com to localhost:3000. The tunnel and DNS record are made for you, and records Teitunnel didn't create are never taken over silently.",
    href: "/docs/getting-started/first-route",
  },
  {
    icon: LockKeyhole,
    title: "Logins in one step",
    body: "Put Cloudflare Access in front of a route: only the people or email domains you list get in, with a one-time code by email.",
    href: "/docs/guides/require-login",
  },
  {
    icon: Network,
    title: "Private networks",
    body: "Let devices running Cloudflare WARP reach a range on your network, with the Split Tunnel settings that would block them checked for you.",
    href: "/docs/guides/private-networks",
  },
  {
    icon: GitBranch,
    title: "Several tunnels",
    body: "Keep staging apart from production on one machine, each with its own connector, or run a tunnel made elsewhere.",
    href: "/docs/guides/several-tunnels",
  },
  {
    icon: Scale,
    title: "Load balancing",
    body: "Serve one hostname from several machines with health checks and failover, the way Cloudflare recommends for tunnels.",
    href: "/docs/guides/load-balancing",
  },
  {
    icon: Zap,
    title: "Always on",
    body: "Keep routes up after you quit and across restarts, as a launchd, systemd or Task Scheduler service.",
    href: "/docs/concepts/run-modes",
  },
  {
    icon: ServerCog,
    title: "Servers and Docker",
    body: "The CLI runs routes on any VPS or cloud VM, as a sandboxed systemd service or in the Docker image, with a web dashboard and an API.",
    href: "/docs/guides/servers",
  },
  {
    icon: Terminal,
    title: "A real CLI",
    body: "Every change from the terminal, with the same plan before it's applied. Scriptable, with JSON output and a health check.",
    href: "/docs/reference/cli",
  },
  {
    icon: Activity,
    title: "Traffic and logs",
    body: "Requests, errors and latency per tunnel, logs per route, and the connectors of every machine in the account.",
    href: "/docs/guides/observability",
  },
  {
    icon: Boxes,
    title: "Import and export",
    body: "Bring existing cloudflared setups in, or export your routes as config.yml, Docker Compose or Terraform.",
    href: "/docs/guides/export",
  },
];

const steps = [
  {
    title: "Install",
    body: "Download the app for macOS, Windows or Linux. It fetches and verifies cloudflared for you.",
  },
  {
    title: "Share or connect",
    body: "Share a port right away with no account, or connect Cloudflare with a token made from a pre-filled template.",
  },
  {
    title: "Review and apply",
    body: "Add a route, read exactly what will change, and apply. Teitunnel checks the URL works when it's done.",
  },
];

const faq: { q: string; a: ReactNode }[] = [
  {
    q: "Is it free?",
    a: "Yes. Teitunnel is free and open source. Cloudflare Tunnel is free too; features such as Load Balancing need Cloudflare's paid add-ons.",
  },
  {
    q: "Do I need a Cloudflare account?",
    a: "Not for Quick Share, which gives you a temporary trycloudflare.com address. Routes on your own domains need a free Cloudflare account with the domain added.",
  },
  {
    q: "Where are my credentials kept?",
    a: "In your operating system's keychain, and only ever sent to Cloudflare. On a server, the API token comes from the environment for each command and is never written to disk.",
  },
  {
    q: "What happens if a change fails halfway?",
    a: "Everything done before the failure is undone, in reverse order. Activity shows what happened, step by step.",
  },
  {
    q: "Can I keep using cloudflared?",
    a: "Yes. Teitunnel uses cloudflared under the hood, can import existing setups, and exports its routes back to config.yml.",
  },
];

export default function Home() {
  return (
    <main className="flex flex-col">
      {/* Hero */}
      <section className="relative overflow-hidden">
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 top-0 h-[520px] bg-[radial-gradient(60%_60%_at_50%_0%,var(--tt-accent),transparent)] opacity-[0.14] dark:opacity-[0.22]"
        />
        <div className="relative mx-auto flex w-full max-w-6xl flex-col items-center px-6 pt-20 text-center md:pt-28">
          <p className="mb-6 rounded-full border border-fd-border px-3 py-1 font-mono text-xs text-fd-muted-foreground">
            macOS · Windows · Linux · servers
          </p>
          <h1 className="max-w-4xl text-4xl font-semibold tracking-tight text-balance md:text-7xl">
            Cloudflare Tunnel, done right.
          </h1>
          <p className="mt-6 max-w-2xl text-lg text-balance text-fd-muted-foreground md:text-xl">
            Share a local port in one click. Publish your apps on your own domains. See every change
            to your Cloudflare account before it happens.
          </p>
          <div className="mt-10 flex flex-wrap items-center justify-center gap-3">
            <ButtonLink href={site.releases} primary external>
              <Download className="size-4" /> Download
            </ButtonLink>
            <ButtonLink href="/docs">
              Read the docs <ArrowRight className="size-4" />
            </ButtonLink>
          </div>
          <p className="mt-4 text-sm text-fd-muted-foreground">Free and open source.</p>
          <div className="mt-16 w-full max-w-5xl md:mt-20">
            <Shot
              name="routes"
              alt="Teitunnel's Routes view: routes grouped by domain, with status, logs and activity"
              priority
            />
          </div>
        </div>
      </section>

      {/* Stories */}
      <Section
        eyebrow="Quick Share"
        title={
          <>
            A public URL for anything on your machine.{" "}
            <span className="text-fd-muted-foreground">No account, no config.</span>
          </>
        }
      >
        <div className="grid items-center gap-10 md:grid-cols-5">
          <div className="space-y-4 text-fd-muted-foreground md:col-span-2">
            <p>
              Pick a running service (dev servers and Docker containers are found for you) and get a
              trycloudflare.com address, a QR code for your phone, and live requests.
            </p>
            <p>
              With a Cloudflare account, share on a subdomain of your own domain instead. It goes
              away when you stop it, when its time is up, or when you quit.
            </p>
          </div>
          <div className="md:col-span-3">
            <Shot name="quick-share" alt="Quick Share with a live share and its URL" />
          </div>
        </div>
      </Section>

      <Section
        eyebrow="Reviewed changes"
        title={
          <>
            Nothing changes until you've seen it.{" "}
            <span className="text-fd-muted-foreground">And nothing is left half done.</span>
          </>
        }
      >
        <div className="grid items-center gap-10 md:grid-cols-5">
          <div className="md:col-span-3 md:order-last">
            <Shot
              name="review/routes"
              alt="Reviewing a new route: every step, and a warning about a DNS record"
            />
          </div>
          <div className="space-y-5 md:col-span-2">
            {[
              {
                icon: ShieldCheck,
                text: "Every change is a plan: the tunnel, DNS records, logins, pools. You read it, then apply it.",
              },
              {
                icon: Undo2,
                text: "If a step fails, the ones before it are undone. Every change can be undone afterwards too.",
              },
              {
                icon: KeyRound,
                text: "Records and apps Teitunnel didn't create are only replaced when you say so.",
              },
            ].map(({ icon: Icon, text }) => (
              <div key={text} className="flex gap-3">
                <Icon className="mt-0.5 size-5 shrink-0 text-[var(--tt-accent)]" />
                <p className="text-fd-muted-foreground">{text}</p>
              </div>
            ))}
          </div>
        </div>
      </Section>

      <Section eyebrow="Doctor" title={<>Problems explained, with the fix one click away.</>}>
        <div className="grid items-center gap-10 md:grid-cols-5">
          <div className="space-y-4 text-fd-muted-foreground md:col-span-2">
            <p>
              The Doctor checks your routes, DNS, connectors, logins and WARP settings, says what's
              wrong in plain words, and fixes what's safe to fix.
            </p>
            <p>
              Missing a permission? It shows exactly what to add to your token, and picks up where
              you left off when you come back.
            </p>
          </div>
          <div className="md:col-span-3">
            <Shot name="doctor" alt="The Doctor listing a problem with its explanation and fix" />
          </div>
        </div>
      </Section>

      {/* Capabilities */}
      <Section eyebrow="Everything in one place" title="From a quick demo to production.">
        <div className="grid gap-px overflow-hidden rounded-2xl border border-fd-border bg-fd-border sm:grid-cols-2 lg:grid-cols-3">
          {capabilities.map(({ icon: Icon, title, body, href }) => (
            <a
              key={title}
              href={`${site.basePath}${href}`}
              className="group flex flex-col gap-3 bg-fd-background p-6 transition-colors hover:bg-fd-card"
            >
              <Icon className="size-5 text-[var(--tt-accent)]" />
              <h3 className="font-medium">{title}</h3>
              <p className="text-sm text-fd-muted-foreground">{body}</p>
            </a>
          ))}
          <a
            href={`${site.basePath}/docs/reference/doctor`}
            className="group flex flex-col gap-3 bg-fd-background p-6 transition-colors hover:bg-fd-card"
          >
            <Stethoscope className="size-5 text-[var(--tt-accent)]" />
            <h3 className="font-medium">Doctor</h3>
            <p className="text-sm text-fd-muted-foreground">
              Checks that explain themselves, safe fixes in one click, and notifications when
              something breaks.
            </p>
          </a>
          <a
            href={`${site.basePath}/docs/reference/security`}
            className="group flex flex-col gap-3 bg-fd-background p-6 transition-colors hover:bg-fd-card"
          >
            <ShieldCheck className="size-5 text-[var(--tt-accent)]" />
            <h3 className="font-medium">Secure by default</h3>
            <p className="text-sm text-fd-muted-foreground">
              Credentials in your keychain, verified cloudflared, no shell commands, and nothing
              deleted that isn't Teitunnel's.
            </p>
          </a>
        </div>
      </Section>

      {/* How it works */}
      <Section eyebrow="Get going" title="Three steps, a few minutes.">
        <ol className="grid gap-6 md:grid-cols-3">
          {steps.map((step, index) => (
            <li key={step.title} className="rounded-2xl border border-fd-border p-6">
              <span className="font-mono text-sm text-fd-muted-foreground">0{index + 1}</span>
              <h3 className="mt-3 text-lg font-medium">{step.title}</h3>
              <p className="mt-2 text-fd-muted-foreground">{step.body}</p>
            </li>
          ))}
        </ol>
      </Section>

      {/* Platforms */}
      <Section eyebrow="Runs where you work" title="On your computer, on your servers.">
        <div className="grid gap-6 md:grid-cols-2">
          <div className="min-w-0 rounded-2xl border border-fd-border p-6">
            <h3 className="text-lg font-medium">Desktop</h3>
            <p className="mt-2 text-fd-muted-foreground">
              A native app for macOS, Windows and Linux, with a menu bar or tray, notifications, and
              your platform's look.
            </p>
            <div className="mt-6">
              <ButtonLink href={site.releases} primary external>
                <Download className="size-4" /> Download
              </ButtonLink>
            </div>
          </div>
          <div className="min-w-0 rounded-2xl border border-fd-border p-6">
            <h3 className="text-lg font-medium">Servers and containers</h3>
            <p className="mt-2 text-fd-muted-foreground">
              The same engine from the command line, as a systemd service, or in Docker.
            </p>
            <pre className="mt-6 overflow-x-auto rounded-xl bg-fd-card p-4 font-mono text-sm">
              <code>{`export CLOUDFLARE_API_TOKEN=…
teitunnel-cli route add app.example.com 3000 --yes
sudo -E teitunnel-cli always-on on`}</code>
            </pre>
          </div>
        </div>
      </Section>

      {/* FAQ */}
      <Section eyebrow="Questions" title="Good to know.">
        <div className="divide-y divide-fd-border rounded-2xl border border-fd-border">
          {faq.map(({ q, a }) => (
            <details key={q} className="group p-6 [&_summary::-webkit-details-marker]:hidden">
              <summary className="flex cursor-pointer list-none items-center justify-between gap-4 font-medium">
                {q}
                <span className="text-fd-muted-foreground transition-transform group-open:rotate-45">
                  +
                </span>
              </summary>
              <p className="mt-3 text-fd-muted-foreground">{a}</p>
            </details>
          ))}
        </div>
      </Section>

      {/* Closing */}
      <section className="mx-auto w-full max-w-6xl px-6 pb-24">
        <div className="rounded-3xl border border-fd-border bg-fd-card px-6 py-16 text-center md:py-20">
          <h2 className="text-3xl font-semibold tracking-tight md:text-5xl">
            Put it on the internet. Safely.
          </h2>
          <p className="mx-auto mt-4 max-w-xl text-fd-muted-foreground">
            Free, open source, and made to feel at home on your platform.
          </p>
          <div className="mt-8 flex flex-wrap justify-center gap-3">
            <ButtonLink href={site.releases} primary external>
              <Download className="size-4" /> Download
            </ButtonLink>
            <ButtonLink href="/docs">
              Read the docs <ArrowRight className="size-4" />
            </ButtonLink>
          </div>
        </div>
      </section>

      <footer className="border-t border-fd-border">
        <div className="mx-auto flex w-full max-w-6xl flex-col gap-4 px-6 py-10 text-sm text-fd-muted-foreground md:flex-row md:items-center md:justify-between">
          <p>Teitunnel is free and open source. Not affiliated with Cloudflare.</p>
          <nav className="flex gap-6">
            <a href={`${site.basePath}/docs`}>Docs</a>
            <a href={site.github} target="_blank" rel="noopener noreferrer">
              GitHub
            </a>
            <a href={site.releases} target="_blank" rel="noopener noreferrer">
              Releases
            </a>
          </nav>
        </div>
      </footer>
    </main>
  );
}
