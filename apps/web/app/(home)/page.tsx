import {
  ArrowRight,
  Beer,
  Blocks,
  Bot,
  Boxes,
  Cable,
  Code2,
  Command,
  Container,
  Download,
  EyeOff,
  GitMerge,
  GitPullRequest,
  Globe,
  HardDrive,
  House,
  KeyRound,
  ListChecks,
  LockKeyhole,
  Monitor,
  PackageCheck,
  ShieldCheck,
  SquareTerminal,
  Undo2,
  Webhook,
} from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import { AppTour, type TourStop } from "@/components/app-tour";
import { CopyCommand } from "@/components/copy-command";
import { DoctorDemo, QuickShareDemo } from "@/components/demos";
import { DownloadButton } from "@/components/download-button";
import { Feature } from "@/components/feature";
import { TunnelFlow } from "@/components/flow";
import { JsonLd } from "@/components/json-ld";
import { ButtonLink, delay, Section, Shot, screens } from "@/components/landing";
import {
  ApprovalDemo,
  CommentDemo,
  ExposureDemo,
  ReplayDemo,
  RequestStream,
} from "@/components/live-demos";
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

const proof = [
  { icon: PackageCheck, title: "Free and open source", body: "MIT licensed. No account with us." },
  { icon: Monitor, title: "Native everywhere", body: "macOS, Windows and Linux, plus a CLI." },
  { icon: Globe, title: "Your own domains", body: "On your own Cloudflare account." },
  { icon: ListChecks, title: "Reviewed first", body: "Nothing changes until you apply it." },
];

const jobs = [
  ["Share", "#share"],
  ["Inspect", "#inspect"],
  ["Publish", "#publish"],
  ["Protect", "#protect"],
  ["Automate", "#automate"],
] as const;

// Cards that float over a screenshot: below it on phones, over its corner on wide screens.
const overlay = {
  left: "relative z-10 -mt-10 flex justify-center px-3 sm:px-8 lg:absolute lg:-bottom-10 lg:-left-10 lg:mt-0 lg:px-0",
  right:
    "relative z-10 -mt-10 flex justify-center px-3 sm:px-8 lg:absolute lg:-right-8 lg:-bottom-10 lg:mt-0 lg:px-0",
};

const snapshotSteps = [
  {
    text: "Upload 3 new or changed files of 48 (212 kB)",
    detail: "files Cloudflare already has aren't sent again",
  },
  { text: "Make the new version live once every file is uploaded", detail: "version 4" },
  {
    text: "Serve preview.teispace.com with the Snapshot",
    detail: "Cloudflare adds its DNS record and certificate",
  },
];

const tour: TourStop[] = [
  {
    name: "routes",
    label: "Routes",
    caption: "Routes grouped by domain, each with its machines, requests, protection and logs.",
    alt: "Routes grouped by domain, with app.teispace.com selected: its address, details, machines and edge protection",
  },
  {
    name: "activity",
    label: "Activity",
    caption: "Every change, what it did in Cloudflare step by step, and a way to undo it.",
    alt: "Activity listing changes, with a rename selected showing its DNS and route changes and steps",
  },
  {
    name: "tunnels",
    label: "Tunnels",
    caption: "Every tunnel in the account, its connectors, private networks and traffic.",
    alt: "The Tunnels view: a tunnel's details, connector, private network and a traffic chart",
  },
  {
    name: "domains",
    label: "Domains",
    caption: "Your Cloudflare domains, what the credential may do on each, and reserved names.",
    alt: "The Domains view with teispace.com selected: status, plan, permissions and hostname reservations",
  },
  {
    name: "analytics",
    label: "Analytics",
    caption:
      "Requests, server errors, response times and uptime for every route, from Cloudflare's edge.",
    alt: "Analytics: every route with a sparkline, request count, server errors, 95th percentile and uptime",
  },
  {
    name: "local-domains",
    label: "Local Domains",
    caption: "Real HTTPS addresses for services on this computer, trusted by your browsers.",
    alt: "Local Domains: api.teispace.localhost with its address, details and request recording",
  },
  {
    name: "projects",
    label: "Projects",
    caption: "A teitunnel.yml in your repository, checked and applied like any other change.",
    alt: "Projects: a project file's routes, share, Snapshot and local domain, applied or missing",
  },
];

const tools = [
  { icon: Code2, name: "VS Code", note: "Cursor, Windsurf" },
  { icon: Blocks, name: "JetBrains IDEs", note: "IntelliJ, WebStorm…" },
  { icon: Command, name: "Raycast", note: "share from anywhere" },
  { icon: GitPullRequest, name: "GitHub Action", note: "pull request previews" },
  { icon: GitMerge, name: "GitLab CI", note: "merge request previews" },
  { icon: Container, name: "Docker", note: "image and Compose" },
  { icon: Beer, name: "Homebrew", note: "app and CLI" },
  { icon: Bot, name: "MCP", note: "8 AI tools, one click" },
];

const security = [
  {
    icon: KeyRound,
    title: "Credentials in your keychain",
    body: "API and OAuth tokens live in the system keychain and are only sent to Cloudflare. Tunnel tokens never appear in a command line.",
  },
  {
    icon: EyeOff,
    title: "No telemetry",
    body: "No analytics, no crash reporting and no account with us. Captured requests stay on your computer.",
  },
  {
    icon: Undo2,
    title: "Reviewed, then undoable",
    body: "Every change to Cloudflare is a plan you read first. A step that fails undoes the ones before it.",
  },
  {
    icon: ShieldCheck,
    title: "Nothing of yours is overwritten",
    body: "DNS records and apps Teitunnel didn't create are only replaced when you say so, and never deleted by it.",
  },
  {
    icon: LockKeyhole,
    title: "Secrets masked",
    body: "Authorization headers, cookies, API keys and webhook signatures are masked in the inspector and exports.",
  },
  {
    icon: SquareTerminal,
    title: "No shells, verified binaries",
    body: "cloudflared is downloaded from Cloudflare and verified, and every process starts without a shell. macOS builds are signed and notarized.",
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
    body: "Point Stripe, GitHub or Slack at your laptop with a stable address, and replay deliveries while you debug.",
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
    a: "A free, open-source app for Cloudflare Tunnel. It shares anything running on your computer at a public URL, publishes your apps on your own domains, shows every request that reaches them, and protects them with logins and edge rules, without port forwarding or editing cloudflared config files.",
  },
  {
    q: "Is it free?",
    a: "Yes. Teitunnel is free and open source (MIT). Cloudflare Tunnel is free on Cloudflare's free plan too; a few features, such as load balancing or per-hostname rate limits, need a paid Cloudflare plan or add-on, and Teitunnel says so where it matters.",
  },
  {
    q: "Do I need a Cloudflare account?",
    a: "Not for Quick Share, which gives you a temporary trycloudflare.com address, or for local domains. Routes, Snapshots and logins need a free Cloudflare account with your domain added.",
  },
  {
    q: "Does anything run on Teitunnel's servers?",
    a: "No. There are none. Tunnels, Snapshots, offline pages and webhook inboxes live on your own Cloudflare account, and captured requests stay on your computer.",
  },
  {
    q: "Does it replace cloudflared?",
    a: "No, it runs it. Teitunnel downloads and verifies Cloudflare's cloudflared, or uses the one you have, imports existing config.yml setups, and exports its routes back to config.yml, Docker Compose or Terraform.",
  },
  {
    q: "What can an AI agent do, and what does it see?",
    a: "Through Teitunnel's MCP server an agent can share a dev server, add routes, read logs and replay webhooks. Every change waits for your approval unless you say otherwise, and API, OAuth and tunnel tokens never reach it.",
  },
  {
    q: "What happens if a change fails halfway?",
    a: "Everything done before the failure is undone, in reverse order. Activity shows what happened, step by step, and lets you undo changes that went through.",
  },
];

// What the CLI prints (apps/cli/src/main.rs, locales/en.json).
const cli = [
  "$ export CLOUDFLARE_API_TOKEN=…",
  "$ teitunnel route add app.teispace.com 3000 --yes",
  " 1. Create tunnel “web-01”",
  " 2. Update tunnel “web-01” to serve 1 route",
  " 3. Add DNS record app.teispace.com → tunnel “web-01”",
  "    done: Create tunnel “web-01”",
  "    done: Update tunnel “web-01” to serve 1 route",
  "    done: Add DNS record app.teispace.com → tunnel “web-01”",
  "Checking https://app.teispace.com…",
  "https://app.teispace.com works.",
  "$ sudo -E teitunnel always-on on",
  "Turning Always-on on for web-01…",
  "Done: the connectors run as a service and start again after a restart.",
];

export default async function Home() {
  const release = await latestRelease();
  const downloads = release?.downloads ?? [];
  return (
    <main className="flex flex-col overflow-x-clip">
      <JsonLd things={[organization, website, softwareApplication(release), faqPage(faq)]} />

      {/* Hero: it runs up under the transparent header. */}
      <section className="relative -mt-14 pt-14">
        <div className="relative mx-auto flex w-full max-w-6xl flex-col items-center px-4 pt-16 text-center sm:px-6 md:pt-24">
          <a
            data-hero
            href={site.releases}
            target="_blank"
            rel="noopener noreferrer"
            className="group mb-7 inline-flex max-w-full items-center gap-2 rounded-full border border-fd-border bg-fd-background py-1 ps-1.5 pe-3 text-xs transition-colors hover:bg-fd-accent"
          >
            <span className="shrink-0 rounded-full bg-fd-foreground px-2 py-0.5 font-medium text-fd-background">
              New in 0.3.0
            </span>
            <span className="truncate text-fd-muted-foreground">
              Self-healing tunnels, breakpoints, MCP sign-in
            </span>
            <ArrowRight
              className="size-3.5 shrink-0 text-fd-muted-foreground transition-transform group-hover:translate-x-0.5"
              aria-hidden
            />
          </a>
          <h1
            data-hero
            style={delay(80)}
            className="max-w-4xl text-[2.6rem] leading-[1.05] font-semibold tracking-tight text-balance sm:text-6xl md:text-7xl"
          >
            Share, publish and protect your local work.
          </h1>
          <p
            data-hero
            style={delay(160)}
            className="mt-6 max-w-2xl text-lg text-balance text-fd-muted-foreground md:text-xl"
          >
            A free, native app for Cloudflare Tunnel. A public URL in one click, your own domains
            with every change reviewed, and an inspector for every request. On your own Cloudflare
            account.
          </p>
          <div
            data-hero
            style={delay(240)}
            className="mt-9 flex flex-col items-center gap-4 sm:flex-row sm:gap-3"
          >
            <DownloadButton downloads={downloads} className="justify-center" />
          </div>
          <div data-hero style={delay(300)} className="mt-4 flex max-w-full justify-center">
            <CopyCommand
              command="brew install --cask teispace/tap/teitunnel"
              label="Copy the Homebrew command"
            />
          </div>
          <div data-hero style={delay(380)} className="mt-14 w-full max-w-5xl md:mt-20">
            <Shot
              name="overview"
              alt="Teitunnel's Overview: a problem banner, a traffic chart, routes on teispace.com and teispace.dev, and live Quick Shares"
              priority
              className="text-left"
            >
              <div className={overlay.right}>
                <RequestStream />
              </div>
            </Shot>
          </div>
        </div>
      </section>

      {/* Proof strip */}
      <section aria-label="In short" className="mx-auto mt-24 w-full max-w-6xl px-4 sm:px-6">
        <ul className="grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-fd-border bg-fd-border lg:grid-cols-4">
          {proof.map(({ icon: Icon, title, body }, index) => (
            <li
              key={title}
              data-reveal
              style={delay(index * 70)}
              className="flex flex-col gap-2 bg-fd-background p-5 md:p-6"
            >
              <Icon className="size-5 text-[var(--tt-accent-text)]" aria-hidden />
              <p className="font-medium">{title}</p>
              <p className="text-sm text-fd-muted-foreground">{body}</p>
            </li>
          ))}
        </ul>
      </section>

      {/* Feature tour, by job */}
      <Section
        id="features"
        eyebrow="Everything in one app"
        title="From a quick demo to a site that stays up."
        lead="Five jobs developers do with local work, each a click away and each on your own Cloudflare account."
      >
        <nav aria-label="Features" className="-mt-4 mb-16 flex flex-wrap gap-2 md:mb-24">
          {jobs.map(([label, href]) => (
            <a
              key={href}
              href={href}
              className="inline-flex h-9 items-center rounded-full border border-fd-border px-4 text-sm transition-colors hover:bg-fd-accent"
            >
              {label}
            </a>
          ))}
        </nav>

        <div className="flex flex-col gap-28 md:gap-36">
          <Feature
            id="share"
            eyebrow="Share"
            title="A public URL for anything on your machine."
            lead="Pick a running service and get an HTTPS address in seconds. No account needed; with one, share on your own domain."
            bullets={[
              <>
                <strong>Found for you:</strong> running dev servers and containers are listed, and a
                dev server that refuses the public address comes with the fix.
              </>,
              <>
                <strong>Your rules:</strong> a timer, a schedule, pause without losing the address,
                a QR code for your phone, or a whole folder instead of a server.
              </>,
              <>
                <strong>Local HTTPS domains:</strong> <code>shop.test</code> or{" "}
                <code>app.localhost</code> with certificates your browsers trust, and{" "}
                <code>.local</code> names for phones on your Wi-Fi.
              </>,
              <>
                <strong>Checked before it's public:</strong> a served <code>.env</code> file, the
                git folder or a debug page is pointed out first.
              </>,
            ]}
            links={[
              ["Quick Share", "/docs/getting-started/quick-share/"],
              ["Sharing extras", "/docs/guides/sharing/"],
              ["Local domains", "/docs/guides/local-domains/"],
            ]}
            media={
              <Shot
                name="quick-share"
                alt="Quick Share: shares on teispace.com with pause and schedule, and a live trycloudflare.com share with its request count"
              >
                <div className={overlay.left}>
                  <QuickShareDemo />
                </div>
              </Shot>
            }
          />

          <Feature
            id="inspect"
            eyebrow="Inspect"
            title="See every request. Replay any of them."
            lead="Requests to your shares and routes stream through Teitunnel's inspector on your computer, untouched and never sent anywhere."
            bullets={[
              <>
                <strong>Headers, bodies and timing</strong> for every request, WebSocket message and
                event stream, with credentials masked.
              </>,
              <>
                <strong>Webhooks recognised:</strong> Stripe, GitHub, Slack, Shopify and more, with
                signatures checked. Replay them, edited or re-signed.
              </>,
              <>
                <strong>Break things on purpose:</strong> 3G or satellite latency, 500s and resets
                on chosen paths, stubs while your service restarts.
              </>,
              <>
                <strong>Export</strong> as cURL, HAR or Markdown, draft an OpenAPI description, and
                follow each route's traffic, errors and uptime in Analytics.
              </>,
            ]}
            links={[
              ["Inspector", "/docs/guides/inspector/"],
              ["OpenAPI from traffic", "/docs/guides/openapi/"],
              ["Analytics and uptime", "/docs/guides/analytics/"],
            ]}
            media={
              <Shot
                name="inspector"
                alt="The Inspector: a list of requests with method, path, status, duration and size, and one request's summary, timing and headers"
              >
                <div className={overlay.right}>
                  <ReplayDemo />
                </div>
              </Shot>
            }
          />

          <Feature
            id="publish"
            eyebrow="Publish"
            title="Your own domains, with nothing changed behind your back."
            lead="Route app.teispace.com to localhost:3000. The tunnel and DNS record are made for you, as a plan you read before it's applied."
            bullets={[
              <>
                <strong>Every change is a plan:</strong> tunnels, DNS records, logins and pools. If
                a step fails, the ones before it are undone.
              </>,
              <>
                <strong>Activity keeps the history,</strong> step by step, with each change undoable
                afterwards.
              </>,
              <>
                <strong>Grows with you:</strong> several tunnels, load balancing across machines,
                private networks, and routes that stay up after you quit.
              </>,
            ]}
            links={[
              ["Your first route", "/docs/getting-started/first-route/"],
              ["Several tunnels", "/docs/guides/several-tunnels/"],
              ["Load balancing", "/docs/guides/load-balancing/"],
            ]}
            media={
              <Shot
                name="review"
                alt="Reviewing a new route for shop.teispace.app: a warning about an existing DNS record and the three steps of the change"
              >
                <div className={overlay.left}>
                  <div className="w-full max-w-sm">
                    <PlanDemo />
                  </div>
                </div>
              </Shot>
            }
          />

          <Feature
            eyebrow="Publish"
            title="Online while your computer sleeps."
            lead="Some things should stay up when you close the lid. Teitunnel puts them on your Cloudflare account, as Workers you own."
            bullets={[
              <>
                <strong>Snapshots:</strong> a static copy of a folder, a build or a running site,
                with versions, rollback, a password or a login.
              </>,
              <>
                <strong>Comments:</strong> reviewers pin notes to any spot on a Snapshot or share;
                you answer them in the app.
              </>,
              <>
                <strong>Offline page and webhook inbox:</strong> your own "back soon" page instead
                of an error, and webhooks kept until you're back.
              </>,
            ]}
            links={[
              ["Snapshots", "/docs/guides/snapshots/"],
              ["Comments", "/docs/guides/comments/"],
              ["Offline page", "/docs/guides/offline-page/"],
              ["Webhook inbox", "/docs/guides/webhook-inbox/"],
            ]}
            media={
              <div className="grid gap-6">
                <Shot
                  name="snapshots"
                  alt="Snapshots: the Launch page Snapshot at preview.teispace.com with its details and three versions"
                >
                  <div className={overlay.right}>
                    <div className="w-full max-w-sm">
                      <PlanDemo
                        title="Publish version 4 of Launch page"
                        steps={snapshotSteps}
                        done="preview.teispace.com"
                        doneLabel="Online"
                      />
                    </div>
                  </div>
                </Shot>
              </div>
            }
          />

          <Feature
            eyebrow="Publish"
            title="Feedback exactly where it applies."
            lead="Send a link and reviewers click the spot they mean. Threads, replies and resolved notes come back to Teitunnel's Comments view."
            bullets={[
              <>
                <strong>On Snapshots and shares,</strong> with a small comment button on the page
                and nothing for reviewers to install.
              </>,
              <>
                <strong>Signed in when it matters:</strong> behind a login, comments carry the
                reviewer's Cloudflare Access email.
              </>,
              <>
                <strong>Answer from anywhere:</strong> the app, the terminal or an AI agent.
              </>,
            ]}
            links={[["Comments", "/docs/guides/comments/"]]}
            media={
              <Shot
                name="comments"
                alt="Comments: open threads on the Launch Snapshot, with a reviewer's note, the owner's reply and a signed-in follow-up"
              >
                <div className={overlay.left}>
                  <CommentDemo />
                </div>
              </Shot>
            }
          />

          <Feature
            id="protect"
            eyebrow="Protect"
            title="Decide who gets in, at Cloudflare's edge."
            lead="Rules Cloudflare enforces for one hostname, so they keep working while your computer is off and blocked traffic never reaches it."
            bullets={[
              <>
                <strong>Logins:</strong> only the people or email domains you list, with a one-time
                code by email, through Cloudflare Access.
              </>,
              <>
                <strong>Bots and crawlers:</strong> challenge or block automated clients and AI
                crawlers, limit requests per visitor, set headers.
              </>,
              <>
                <strong>Service tokens</strong> let CI jobs and scripts through a login with two
                headers.
              </>,
              <>
                <strong>Quick Shares too:</strong> a password, a secret link or an IP allow list,
                enforced by the inspector on your computer.
              </>,
            ]}
            links={[
              ["Require a login", "/docs/guides/require-login/"],
              ["Edge protection", "/docs/guides/protection/"],
              ["Exposure check", "/docs/guides/exposure-check/"],
            ]}
            media={
              <Shot
                name="protection"
                alt="Protect app.teispace.com: automated clients set to allow, block AI crawlers, rate limit and header rules"
              >
                <div className={overlay.right}>
                  <ExposureDemo />
                </div>
              </Shot>
            }
          />

          <Feature
            eyebrow="Fix"
            title="Problems explained, with the fix one click away."
            lead="The Doctor checks routes, DNS, connectors, logins and WARP settings, says what's wrong in plain words, and fixes what's safe to fix."
            bullets={[
              <>
                <strong>Missing a permission?</strong> It shows exactly what to add to your token
                and picks up where you left off.
              </>,
              <>
                <strong>Inline everywhere:</strong> issues show on the route or tunnel they concern,
                and in the menu bar.
              </>,
              <>
                <strong>Notifications</strong> when a route goes down, errors or slows down, with
                quiet hours.
              </>,
            ]}
            links={[
              ["Doctor", "/docs/reference/doctor/"],
              ["Cloudflare errors, explained", "/docs/reference/troubleshooting/"],
            ]}
            media={
              <Shot
                name="doctor"
                alt="The Doctor: docs.teispace.com has no DNS record, with its explanation and a Fix the DNS Record button"
              >
                <div className={overlay.left}>
                  <DoctorDemo />
                </div>
              </Shot>
            }
          />

          <Feature
            id="automate"
            eyebrow="Automate"
            title="The same engine in your repository, terminal and CI."
            lead="Declare what a project needs once. Everyone on the team, and every pull request, gets the same setup with their own addresses."
            bullets={[
              <>
                <strong>teitunnel.yml:</strong> routes, shares, Snapshots and local domains, applied
                with <code>teitunnel up</code> or one click, and a no-op when nothing changed.
              </>,
              <>
                <strong>Pull request previews</strong> on your domain from the GitHub Action or
                GitLab CI template, removed when the pull request closes.
              </>,
              <>
                <strong>A real CLI</strong> with JSON output, plus <code>teitunnel serve</code> for
                servers: a sandboxed service, Docker image and web dashboard.
              </>,
            ]}
            links={[
              ["Project files", "/docs/guides/project-file/"],
              ["CI previews", "/docs/guides/ci-previews/"],
              ["Servers and Docker", "/docs/guides/servers/"],
            ]}
            media={
              <div className="relative">
                <Shot
                  name="projects"
                  alt="Projects: the shop project's teitunnel.yml with routes, a share, a Snapshot and a local domain, applied or missing"
                />
                <div className="relative z-10 -mt-10 px-3 sm:px-8 lg:absolute lg:-right-8 lg:-bottom-12 lg:mt-0 lg:w-[56%] lg:px-0">
                  <Terminal title="web-01 — ssh" lines={cli} />
                </div>
              </div>
            }
          />
        </div>
      </Section>

      {/* AI agents */}
      <Section
        id="agents"
        eyebrow="Built for AI agents"
        title={
          <>
            Let your agent share and fix things.{" "}
            <span className="text-fd-muted-foreground">You still say yes.</span>
          </>
        }
      >
        <div className="grid grid-cols-1 items-center gap-12 lg:grid-cols-2">
          <div className="space-y-5">
            {[
              {
                icon: Bot,
                text: "Connect Claude Code, Claude Desktop, Cursor, VS Code, Codex, Windsurf, Zed or Gemini CLI in one click, from Settings or the terminal.",
              },
              {
                icon: ListChecks,
                text: "Agents share dev servers, add routes, read logs, wait for a webhook and replay it, through the same plans as the app.",
              },
              {
                icon: ShieldCheck,
                text: "Every change waits for your approval in the app, unless you allow a tool, and shows up in Activity with its name.",
              },
              {
                icon: KeyRound,
                text: "API, OAuth and tunnel tokens never reach the agent, and captured credentials stay masked.",
              },
            ].map(({ icon: Icon, text }, index) => (
              <div key={text} data-reveal style={delay(index * 80)} className="flex gap-3">
                <Icon className="mt-0.5 size-5 shrink-0 text-[var(--tt-accent-text)]" aria-hidden />
                <p className="text-fd-muted-foreground">{text}</p>
              </div>
            ))}
            <Link
              href="/docs/guides/ai-agents/"
              className="inline-flex items-center gap-1 text-sm font-medium underline-offset-4 hover:underline"
            >
              AI agents (MCP) <ArrowRight className="size-3.5" aria-hidden />
            </Link>
          </div>
          <div data-reveal style={delay(120)} className="mx-auto w-full max-w-xl">
            <Shot
              name="agents"
              alt="Settings, AI Tools: Claude Code connected; Cursor, VS Code and others ready to connect; the MCP command for other tools"
              size={screens.settings}
              lights="settings"
            >
              <div className={overlay.left}>
                <ApprovalDemo />
              </div>
            </Shot>
          </div>
        </div>
      </Section>

      {/* Tools */}
      <Section
        id="integrations"
        eyebrow="Works with your tools"
        title="Share from your editor. Preview from CI."
        lead="Extensions and launchers talk to the running app over a local connection. Nothing listens on the network, and nothing changes without your approval."
      >
        <div className="grid grid-cols-1 items-center gap-12 lg:grid-cols-2">
          <ul className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            {tools.map(({ icon: Icon, name, note }, index) => (
              <li
                key={name}
                data-reveal
                style={delay((index % 2) * 60 + Math.floor(index / 2) * 50)}
                className="flex items-center gap-3 rounded-xl border border-fd-border p-3.5"
              >
                <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-fd-card">
                  <Icon className="size-4.5 text-fd-foreground" aria-hidden />
                </span>
                <span className="min-w-0">
                  <span className="block text-sm font-medium">{name}</span>
                  <span className="block truncate text-xs text-fd-muted-foreground">{note}</span>
                </span>
              </li>
            ))}
          </ul>
          <div data-reveal style={delay(120)} className="mx-auto w-full max-w-xl">
            <Shot
              name="integrations"
              alt="Settings, Integrations: allow connections from the command line and extensions, with VS Code and Raycast always allowed"
              size={screens.settingsShort}
              lights="settings"
            />
            <p className="mt-5 text-sm text-fd-muted-foreground">
              A global shortcut shares your dev server from anywhere, and <code>teitunnel://</code>{" "}
              links open a route or a share.{" "}
              <Link
                href="/docs/guides/integrations/"
                className="font-medium text-fd-foreground underline-offset-4 hover:underline"
              >
                Links and integrations
              </Link>
            </p>
          </div>
        </div>
      </Section>

      {/* App tour */}
      <Section
        id="tour"
        eyebrow="A native app"
        title="Made to feel at home on your computer."
        lead="System fonts, your accent color, light and dark, keyboard shortcuts and a menu bar or tray. The same app on macOS, Windows and Linux."
      >
        <div data-reveal>
          <AppTour stops={tour} />
        </div>
      </Section>

      {/* Security and privacy */}
      <Section
        id="security"
        eyebrow="Security and privacy"
        title="Your account, your keys, your computer."
      >
        <div className="grid grid-cols-1 items-start gap-12 lg:grid-cols-12">
          <ul className="grid gap-px overflow-hidden rounded-2xl border border-fd-border bg-fd-border sm:grid-cols-2 lg:col-span-7">
            {security.map(({ icon: Icon, title, body }, index) => (
              <li
                key={title}
                data-reveal
                style={delay((index % 2) * 70)}
                className="flex flex-col gap-2 bg-fd-background p-6"
              >
                <Icon className="size-5 text-[var(--tt-accent-text)]" aria-hidden />
                <p className="font-medium">{title}</p>
                <p className="text-sm text-fd-muted-foreground">{body}</p>
              </li>
            ))}
          </ul>
          <div data-reveal style={delay(120)} className="lg:col-span-5">
            <Shot
              name="accounts"
              alt="Settings, Accounts: the Teispace account connected with an API token, and the permissions it grants on each domain"
              size={screens.accounts}
              lights="settings"
            />
            <p className="mt-5 text-sm text-fd-muted-foreground">
              Teitunnel asks only for the permissions a feature needs, shows what each one is for,
              and checks again when you come back.{" "}
              <Link
                href="/docs/reference/security/"
                className="font-medium text-fd-foreground underline-offset-4 hover:underline"
              >
                Security model
              </Link>{" "}
              ·{" "}
              <Link
                href="/privacy/"
                className="font-medium text-fd-foreground underline-offset-4 hover:underline"
              >
                Privacy
              </Link>
            </p>
          </div>
        </div>
      </Section>

      {/* How it works */}
      <Section id="how-it-works" eyebrow="How it works" title="Three steps, a few minutes.">
        <ol className="mb-10 grid gap-4 md:grid-cols-3">
          {[
            {
              title: "Install",
              body: "Download the app for macOS, Windows or Linux. It fetches and verifies cloudflared for you.",
            },
            {
              title: "Share or connect",
              body: "Share a port right away with no account, or sign in to Cloudflare and grant only the permissions you need.",
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
              className="flex gap-4 rounded-2xl border border-fd-border p-6"
            >
              <span className="flex size-8 shrink-0 items-center justify-center rounded-full border border-fd-border font-mono text-sm">
                {index + 1}
              </span>
              <div>
                <h3 className="font-medium">{step.title}</h3>
                <p className="mt-1 text-sm text-fd-muted-foreground">{step.body}</p>
              </div>
            </li>
          ))}
        </ol>
        <div data-reveal>
          <TunnelFlow />
        </div>
      </Section>

      {/* Use cases */}
      <Section
        id="use-cases"
        eyebrow="Use cases"
        title="What people put on the internet with it."
        lead="Step-by-step guides, from a five-minute demo to a home lab."
      >
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {useCases.map(({ icon: Icon, title, body, href }, index) => (
            <Link
              key={title}
              href={href}
              data-reveal
              style={delay((index % 3) * 70)}
              className="group flex flex-col gap-3 rounded-2xl border border-fd-border p-6 transition-[background-color,border-color] duration-300 hover:border-[var(--tt-accent)]/40 hover:bg-fd-card"
            >
              <span className="flex size-10 items-center justify-center rounded-xl border border-fd-border bg-fd-card">
                <Icon className="size-5 text-[var(--tt-accent-text)]" aria-hidden />
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
            <details
              key={q}
              className="group p-5 md:p-6 [&_summary::-webkit-details-marker]:hidden"
            >
              <summary className="flex cursor-pointer list-none items-center justify-between gap-4 font-medium">
                {q}
                <span
                  aria-hidden
                  className="text-xl leading-none text-fd-muted-foreground transition-transform duration-300 group-open:rotate-45"
                >
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
      <section className="mx-auto w-full max-w-6xl px-4 pb-24 sm:px-6">
        <div
          data-reveal
          className="rounded-3xl border border-fd-border bg-fd-card px-6 py-16 text-center md:py-24"
        >
          <h2 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">
            Put it online. Keep it yours.
          </h2>
          <p className="mx-auto mt-4 max-w-xl text-fd-muted-foreground">
            Free, open source, and on your own Cloudflare account. For macOS, Windows, Linux and
            your servers.
          </p>
          <div className="mt-8 flex flex-wrap items-center justify-center gap-3">
            <DownloadButton downloads={downloads} showAlternative={false} />
            <ButtonLink href="/download/">
              <Download className="size-4" aria-hidden /> All downloads
            </ButtonLink>
            <ButtonLink href="/docs/">
              Read the docs <ArrowRight className="size-4" aria-hidden />
            </ButtonLink>
          </div>
        </div>
      </section>
    </main>
  );
}
