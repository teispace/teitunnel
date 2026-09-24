import {
  Activity,
  ArrowRight,
  Boxes,
  Cable,
  Download,
  GitBranch,
  Globe,
  HardDrive,
  House,
  KeyRound,
  LockKeyhole,
  Network,
  Scale,
  ServerCog,
  ShieldCheck,
  Stethoscope,
  Terminal as TerminalIcon,
  Undo2,
  Webhook,
  Zap,
} from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import { DoctorDemo, QuickShareDemo } from "@/components/demos";
import { DownloadButton } from "@/components/download-button";
import { JsonLd } from "@/components/json-ld";
import { ButtonLink, delay, Section, Shot } from "@/components/landing";
import { PlanDemo } from "@/components/plan-demo";
import { Terminal } from "@/components/terminal";
import { latestRelease } from "@/lib/release";
import {
  faqPage,
  ogImage,
  organization,
  pageMetadata,
  softwareApplication,
  website,
} from "@/lib/seo";
import { site } from "@/lib/site";

export const metadata: Metadata = pageMetadata({
  description: site.description,
  path: "/",
  image: ogImage(["home"]),
});

const capabilities = [
  {
    icon: Globe,
    title: "Your own domains",
    body: "Route app.example.com to localhost:3000. The tunnel and DNS record are made for you, and records Teitunnel didn't create are never taken over silently.",
    href: "/docs/getting-started/first-route/",
  },
  {
    icon: LockKeyhole,
    title: "Logins in one step",
    body: "Put Cloudflare Access in front of a route: only the people or email domains you list get in, with a one-time code by email.",
    href: "/docs/guides/require-login/",
  },
  {
    icon: Network,
    title: "Private networks",
    body: "Let devices running Cloudflare WARP reach a range on your network, with the Split Tunnel settings that would block them checked for you.",
    href: "/docs/guides/private-networks/",
  },
  {
    icon: GitBranch,
    title: "Several tunnels",
    body: "Keep staging apart from production on one machine, each with its own connector, or run a tunnel made elsewhere.",
    href: "/docs/guides/several-tunnels/",
  },
  {
    icon: Scale,
    title: "Load balancing",
    body: "Serve one hostname from several machines with health checks and failover, the way Cloudflare recommends for tunnels.",
    href: "/docs/guides/load-balancing/",
  },
  {
    icon: Zap,
    title: "Always on",
    body: "Keep routes up after you quit and across restarts, as a launchd, systemd or Task Scheduler service.",
    href: "/docs/concepts/run-modes/",
  },
  {
    icon: ServerCog,
    title: "Servers and Docker",
    body: "The CLI runs routes on any VPS or cloud VM, as a sandboxed systemd service or in the Docker image, with a web dashboard and an API.",
    href: "/docs/guides/servers/",
  },
  {
    icon: TerminalIcon,
    title: "A real CLI",
    body: "Every change from the terminal, with the same plan before it's applied. Scriptable, with JSON output and a health check.",
    href: "/docs/reference/cli/",
  },
  {
    icon: Activity,
    title: "Traffic and logs",
    body: "Requests, errors and latency per tunnel, logs per route, and the connectors of every machine in the account.",
    href: "/docs/guides/observability/",
  },
  {
    icon: Boxes,
    title: "Import and export",
    body: "Bring existing cloudflared setups in, or export your routes as config.yml, Docker Compose or Terraform.",
    href: "/docs/guides/export/",
  },
  {
    icon: Stethoscope,
    title: "Doctor",
    body: "Checks that explain themselves, safe fixes in one click, and notifications when something breaks.",
    href: "/docs/reference/doctor/",
  },
  {
    icon: ShieldCheck,
    title: "Secure by default",
    body: "Credentials in your keychain, verified cloudflared, no shell commands, and nothing deleted that isn't Teitunnel's.",
    href: "/docs/reference/security/",
  },
];

const useCases = [
  {
    icon: Globe,
    title: "Expose localhost",
    body: "Show a dev server to a client or test on your phone: a public HTTPS link in seconds, no port forwarding.",
    href: "/docs/tutorials/expose-localhost/",
  },
  {
    icon: Webhook,
    title: "Receive webhooks",
    body: "Point Stripe, GitHub or Slack at your laptop with a stable address that doesn't change on every restart.",
    href: "/docs/tutorials/webhooks/",
  },
  {
    icon: House,
    title: "Home Assistant",
    body: "Reach your smart home from anywhere without opening your router, behind a login if you like.",
    href: "/docs/tutorials/home-assistant/",
  },
  {
    icon: HardDrive,
    title: "Self-host from home",
    body: "Publish a website, NAS or media app from a home server, even behind CGNAT, with your IP kept private.",
    href: "/docs/tutorials/self-host/",
  },
  {
    icon: Cable,
    title: "SSH from anywhere",
    body: "Reach a machine's SSH, RDP or database through Cloudflare, with no inbound port open.",
    href: "/docs/tutorials/ssh/",
  },
  {
    icon: Boxes,
    title: "Docker Compose stacks",
    body: "Add one service to a Compose file and route hostnames to the containers next to it.",
    href: "/docs/tutorials/docker-compose/",
  },
];

const faq: { q: string; a: string }[] = [
  {
    q: "What is Teitunnel?",
    a: "A free, open-source app for Cloudflare Tunnel. It shares anything running on your computer at a public URL, publishes your apps on your own domains, and keeps them running, without port forwarding or editing cloudflared config files.",
  },
  {
    q: "Is it free?",
    a: "Yes. Teitunnel is free and open source (MIT). Cloudflare Tunnel is free on Cloudflare's free plan too; features such as Load Balancing need Cloudflare's paid add-ons.",
  },
  {
    q: "Do I need a Cloudflare account?",
    a: "Not for Quick Share, which gives you a temporary trycloudflare.com address. Routes on your own domains need a free Cloudflare account with the domain added.",
  },
  {
    q: "How is it different from ngrok?",
    a: "Teitunnel runs on Cloudflare's network with your own Cloudflare account: your domains, your DNS, Cloudflare Access for logins, and no separate subscription. It adds a native app, a reviewed plan for every change and a Doctor on top of Cloudflare's own connector.",
  },
  {
    q: "Does it replace cloudflared?",
    a: "No, it runs it. Teitunnel downloads and verifies Cloudflare's cloudflared, or uses the one you have, imports existing config.yml setups, and exports its routes back to config.yml, Docker Compose or Terraform.",
  },
  {
    q: "Where are my credentials kept?",
    a: "In your operating system's keychain, and only ever sent to Cloudflare. On a server, the API token comes from the environment for each command and is never written to disk.",
  },
  {
    q: "What happens if a change fails halfway?",
    a: "Everything done before the failure is undone, in reverse order. Activity shows what happened, step by step.",
  },
];

// What the CLI prints (apps/cli/src/main.rs, locales/en.json).
const cli = [
  "$ export CLOUDFLARE_API_TOKEN=…",
  "$ teitunnel route add app.example.com 3000 --yes",
  " 1. Create tunnel “web-01”",
  " 2. Update tunnel “web-01” to serve 1 route",
  " 3. Add DNS record app.example.com → tunnel “web-01”",
  "    done: Create tunnel “web-01”",
  "    done: Update tunnel “web-01” to serve 1 route",
  "    done: Add DNS record app.example.com → tunnel “web-01”",
  "Checking https://app.example.com…",
  "https://app.example.com works.",
  "$ sudo -E teitunnel always-on on",
  "Turning Always-on on for web-01…",
  "Done: the connectors run as a service and start again after a restart.",
];

export default async function Home() {
  const release = await latestRelease();
  const downloads = release?.downloads ?? [];
  return (
    <main className="flex flex-col">
      <JsonLd things={[organization, website, softwareApplication(release), faqPage(faq)]} />

      {/* Hero: it runs up under the transparent header. */}
      <section className="relative -mt-14 overflow-hidden pt-14">
        <div aria-hidden="true" className="pointer-events-none absolute inset-0">
          <div className="tt-glow absolute -top-40 left-1/2 h-[640px] w-[1100px] -translate-x-1/2 rounded-full bg-[radial-gradient(closest-side,var(--tt-accent),transparent)] opacity-[0.16] blur-2xl dark:opacity-[0.26]" />
          <div className="absolute inset-0 bg-[linear-gradient(to_right,var(--color-fd-border)_1px,transparent_1px),linear-gradient(to_bottom,var(--color-fd-border)_1px,transparent_1px)] bg-[size:56px_56px] opacity-40 [mask-image:radial-gradient(ellipse_70%_50%_at_50%_0%,black,transparent)]" />
        </div>
        <div className="relative mx-auto flex w-full max-w-6xl flex-col items-center px-6 pt-20 text-center md:pt-28">
          <Link
            data-hero
            href={release ? "/download/" : site.github}
            className="group mb-7 inline-flex items-center gap-2 rounded-full border border-fd-border bg-fd-background/70 py-1 ps-1.5 pe-3 text-xs backdrop-blur transition-colors hover:bg-fd-accent"
          >
            <span className="rounded-full bg-fd-foreground px-2 py-0.5 font-medium text-fd-background">
              {release ? `Beta ${release.version}` : "Beta"}
            </span>
            <span className="text-fd-muted-foreground">macOS · Windows · Linux · servers</span>
            <ArrowRight
              className="size-3.5 text-fd-muted-foreground transition-transform group-hover:translate-x-0.5"
              aria-hidden
            />
          </Link>
          <h1
            data-hero
            style={delay(80)}
            className="max-w-4xl text-5xl font-semibold tracking-tight text-balance md:text-7xl"
          >
            Cloudflare Tunnel, <span className="text-[var(--tt-accent)]">done right.</span>
          </h1>
          <p
            data-hero
            style={delay(160)}
            className="mt-6 max-w-2xl text-lg text-balance text-fd-muted-foreground md:text-xl"
          >
            Share a local port in one click. Publish your apps on your own domains. See every change
            to your Cloudflare account before it happens.
          </p>
          <div
            data-hero
            style={delay(240)}
            className="mt-10 flex flex-wrap items-center justify-center gap-3"
          >
            <DownloadButton downloads={downloads} showAlternative={false} />
            <ButtonLink href="/docs/getting-started/install/">
              Get started <ArrowRight className="size-4" />
            </ButtonLink>
          </div>
          <p
            data-hero
            style={delay(300)}
            className="mt-5 flex flex-wrap items-center justify-center gap-x-4 gap-y-1 text-sm text-fd-muted-foreground"
          >
            <span className="inline-flex items-center gap-1.5">
              <ShieldCheck className="size-4" aria-hidden /> Signed and notarized
            </span>
            <span className="inline-flex items-center gap-1.5">
              <KeyRound className="size-4" aria-hidden /> No telemetry
            </span>
            <span className="inline-flex items-center gap-1.5">
              <GitBranch className="size-4" aria-hidden /> Free and open source
            </span>
          </p>
          <div data-hero style={delay(380)} className="mt-16 w-full max-w-5xl md:mt-20">
            <div className="tt-settle">
              <Shot
                name="routes"
                alt="Teitunnel's Routes view: routes grouped by domain, with status, logs and activity"
                priority
              />
            </div>
          </div>
        </div>
      </section>

      {/* How it works */}
      <Section id="how-it-works" eyebrow="How it works" title="Three steps, a few minutes.">
        <div className="grid grid-cols-1 items-center gap-10 md:grid-cols-2">
          <ol className="flex flex-col gap-4">
            {[
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
            ].map((step, index) => (
              <li
                key={step.title}
                data-reveal
                style={delay(index * 100)}
                className="flex gap-5 rounded-2xl border border-fd-border p-6"
              >
                <span className="flex size-9 shrink-0 items-center justify-center rounded-full border border-fd-border font-mono text-sm">
                  {index + 1}
                </span>
                <div>
                  <h3 className="text-lg font-medium">{step.title}</h3>
                  <p className="mt-1 text-fd-muted-foreground">{step.body}</p>
                </div>
              </li>
            ))}
          </ol>
          <div data-reveal style={delay(150)}>
            <PlanDemo />
          </div>
        </div>
      </Section>

      {/* Platforms */}
      <Section
        id="platforms"
        eyebrow="Runs where you work"
        title="On your computer, on your servers."
      >
        <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
          <div
            data-reveal
            className="flex min-w-0 flex-col rounded-2xl border border-fd-border p-6 md:p-8"
          >
            <h3 className="text-lg font-medium">Desktop</h3>
            <p className="mt-2 text-fd-muted-foreground">
              A native app for macOS, Windows and Linux, with a menu bar or tray, notifications, and
              your platform's look. It updates itself and verifies every update.
            </p>
            <ul className="mt-6 grid gap-2 text-sm text-fd-muted-foreground">
              <li>macOS 14 or later · Apple silicon and Intel · signed and notarized</li>
              <li>Windows 10 and 11 · x64 and Arm</li>
              <li>Linux · .deb, .rpm and AppImage · x64 and Arm64</li>
            </ul>
            <div className="mt-8">
              <ButtonLink href="/download/" primary>
                <Download className="size-4" /> All downloads
              </ButtonLink>
            </div>
          </div>
          <div data-reveal style={delay(120)} className="flex min-w-0 flex-col gap-4">
            <Terminal title="web-01 — ssh" lines={cli} />
            <p className="text-sm text-fd-muted-foreground">
              The same engine as a CLI for VPSs, cloud VMs, Docker and Kubernetes, with a web
              dashboard and an API.{" "}
              <Link
                href="/docs/guides/servers/"
                className="font-medium text-fd-foreground underline-offset-4 hover:underline"
              >
                Servers and containers
              </Link>
            </p>
          </div>
        </div>
      </Section>

      {/* Stories */}
      <Section
        id="features"
        eyebrow="Quick Share"
        title={
          <>
            A public URL for anything on your machine.{" "}
            <span className="text-fd-muted-foreground">No account, no config.</span>
          </>
        }
      >
        <div className="grid grid-cols-1 items-center gap-10 md:grid-cols-5">
          <div data-reveal className="space-y-4 text-fd-muted-foreground md:col-span-2">
            <p>
              Pick a running service (dev servers and Docker containers are found for you) and get a
              trycloudflare.com address, a QR code for your phone, and live requests.
            </p>
            <p>
              With a Cloudflare account, share on a subdomain of your own domain instead. It goes
              away when you stop it, when its time is up, or when you quit.
            </p>
            <Link
              href="/docs/getting-started/quick-share/"
              className="inline-flex items-center gap-1 font-medium text-fd-foreground underline-offset-4 hover:underline"
            >
              How Quick Share works <ArrowRight className="size-4" aria-hidden />
            </Link>
          </div>
          <div data-reveal style={delay(120)} className="relative md:col-span-3">
            <Shot name="quick-share" alt="Quick Share with a live share and its URL" />
            <div className="-mt-10 flex justify-center px-4 md:absolute md:-bottom-10 md:-left-10 md:mt-0 md:px-0">
              <QuickShareDemo />
            </div>
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
        <div className="grid grid-cols-1 items-center gap-10 md:grid-cols-5">
          <div data-reveal style={delay(120)} className="md:order-last md:col-span-3">
            <Shot
              name="review"
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
            ].map(({ icon: Icon, text }, index) => (
              <div key={text} data-reveal style={delay(index * 90)} className="flex gap-3">
                <Icon className="mt-0.5 size-5 shrink-0 text-[var(--tt-accent)]" />
                <p className="text-fd-muted-foreground">{text}</p>
              </div>
            ))}
          </div>
        </div>
      </Section>

      <Section eyebrow="Doctor" title={<>Problems explained, with the fix one click away.</>}>
        <div className="grid grid-cols-1 items-center gap-10 md:grid-cols-5">
          <div data-reveal className="space-y-4 text-fd-muted-foreground md:col-span-2">
            <p>
              The Doctor checks your routes, DNS, connectors, logins and WARP settings, says what's
              wrong in plain words, and fixes what's safe to fix.
            </p>
            <p>
              Missing a permission? It shows exactly what to add to your token, and picks up where
              you left off when you come back.
            </p>
            <Link
              href="/docs/reference/troubleshooting/"
              className="inline-flex items-center gap-1 font-medium text-fd-foreground underline-offset-4 hover:underline"
            >
              Cloudflare errors, explained <ArrowRight className="size-4" aria-hidden />
            </Link>
          </div>
          <div data-reveal style={delay(120)} className="relative md:col-span-3">
            <Shot name="doctor" alt="The Doctor listing a problem with its explanation and fix" />
            <div className="-mt-10 flex justify-center px-4 md:absolute md:-right-8 md:-bottom-10 md:mt-0 md:px-0">
              <DoctorDemo />
            </div>
          </div>
        </div>
      </Section>

      {/* Capabilities */}
      <Section
        id="capabilities"
        eyebrow="Everything in one place"
        title="From a quick demo to production."
      >
        <div className="grid gap-px overflow-hidden rounded-2xl border border-fd-border bg-fd-border sm:grid-cols-2 lg:grid-cols-3">
          {capabilities.map(({ icon: Icon, title, body, href }, index) => (
            <Link
              key={title}
              href={href}
              data-reveal
              style={delay((index % 3) * 70)}
              className="group relative flex flex-col gap-3 bg-fd-background p-6 transition-colors hover:bg-fd-card"
            >
              <Icon className="size-5 text-[var(--tt-accent)] transition-transform duration-300 group-hover:scale-110" />
              <h3 className="font-medium">{title}</h3>
              <p className="text-sm text-fd-muted-foreground">{body}</p>
              <ArrowRight
                className="absolute top-6 right-6 size-4 -translate-x-1 text-fd-muted-foreground opacity-0 transition-all duration-300 group-hover:translate-x-0 group-hover:opacity-100"
                aria-hidden
              />
            </Link>
          ))}
        </div>
      </Section>

      {/* Use cases */}
      <Section
        id="use-cases"
        eyebrow="Use cases"
        title="What people put on the internet with it."
        lead="Step-by-step guides for the things Cloudflare Tunnel is best at, from a five-minute demo to a home lab."
      >
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {useCases.map(({ icon: Icon, title, body, href }, index) => (
            <Link
              key={title}
              href={href}
              data-reveal
              style={delay((index % 3) * 70)}
              className="group flex flex-col gap-3 rounded-2xl border border-fd-border p-6 transition-[background-color,border-color,transform] duration-300 hover:-translate-y-0.5 hover:border-[var(--tt-accent)]/40 hover:bg-fd-card"
            >
              <span className="flex size-10 items-center justify-center rounded-xl border border-fd-border bg-fd-card">
                <Icon className="size-5 text-[var(--tt-accent)]" />
              </span>
              <h3 className="font-medium">{title}</h3>
              <p className="text-sm text-fd-muted-foreground">{body}</p>
              <span className="mt-auto inline-flex items-center gap-1 pt-2 text-sm font-medium">
                Read the guide
                <ArrowRight
                  className="size-4 transition-transform group-hover:translate-x-0.5"
                  aria-hidden
                />
              </span>
            </Link>
          ))}
        </div>
      </Section>

      {/* FAQ */}
      <Section id="faq" eyebrow="Questions" title="Good to know.">
        <div data-reveal className="divide-y divide-fd-border rounded-2xl border border-fd-border">
          {faq.map(({ q, a }) => (
            <details key={q} className="group p-6 [&_summary::-webkit-details-marker]:hidden">
              <summary className="flex cursor-pointer list-none items-center justify-between gap-4 font-medium">
                {q}
                <span className="text-xl leading-none text-fd-muted-foreground transition-transform duration-300 group-open:rotate-45">
                  +
                </span>
              </summary>
              <p className="mt-3 text-fd-muted-foreground">{a}</p>
            </details>
          ))}
        </div>
        <p className="mt-6 text-sm text-fd-muted-foreground">
          More in the{" "}
          <Link href="/docs/reference/faq/" className="underline underline-offset-4">
            FAQ
          </Link>{" "}
          and{" "}
          <Link href="/docs/reference/troubleshooting/" className="underline underline-offset-4">
            troubleshooting
          </Link>
          .
        </p>
      </Section>

      {/* Closing */}
      <section className="mx-auto w-full max-w-6xl px-6 pb-24">
        <div
          data-reveal
          className="relative overflow-hidden rounded-3xl border border-fd-border bg-fd-card px-6 py-16 text-center md:py-24"
        >
          <div
            aria-hidden="true"
            className="tt-glow pointer-events-none absolute -bottom-40 left-1/2 h-[420px] w-[820px] -translate-x-1/2 rounded-full bg-[radial-gradient(closest-side,var(--tt-accent),transparent)] opacity-[0.14] blur-2xl dark:opacity-[0.22]"
          />
          <div className="relative">
            <h2 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">
              Put it on the internet. Safely.
            </h2>
            <p className="mx-auto mt-4 max-w-xl text-fd-muted-foreground">
              Free, open source, and made to feel at home on your platform.
            </p>
            <div className="mt-8 flex flex-wrap justify-center gap-3">
              <DownloadButton downloads={downloads} showAlternative={false} />
              <ButtonLink href="/docs/">
                Read the docs <ArrowRight className="size-4" />
              </ButtonLink>
            </div>
          </div>
        </div>
      </section>
    </main>
  );
}
