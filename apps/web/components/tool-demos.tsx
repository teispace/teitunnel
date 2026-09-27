import {
  Blocks,
  Check,
  Command,
  Container,
  Copy,
  GitPullRequest,
  Globe,
  LoaderCircle,
  Puzzle,
  Search,
  SquareTerminal,
} from "lucide-react";
import type { ReactNode } from "react";
import { GuideLink } from "./guide-link";
import { Logo } from "./logo";
import type { PickerItem } from "./picker";

// Every label below is the tool's own: commands and messages from integrations/ and the CLI.
const address = "quiet-river-lamp-orbit.trycloudflare.com";

/** A mock's frame: a title bar and the surface below it. */
function Frame({ title, children }: { title: ReactNode; children: ReactNode }) {
  return (
    <div className="tt-mock">
      <div className="tt-mock-bar">
        <span className="flex gap-1.5" aria-hidden>
          <span className="size-2.5 rounded-full bg-fd-border" />
          <span className="size-2.5 rounded-full bg-fd-border" />
          <span className="size-2.5 rounded-full bg-fd-border" />
        </span>
        <span className="mx-auto truncate font-mono">{title}</span>
      </div>
      <div className="tt-mock-body">{children}</div>
    </div>
  );
}

/** Placeholder code: faint bars of different lengths. */
function Code({ widths }: { widths: number[] }) {
  return (
    <div className="flex flex-col gap-2.5 p-4" aria-hidden>
      {widths.map((width, index) => (
        <span
          // biome-ignore lint/suspicious/noArrayIndexKey: fixed decoration
          key={index}
          className="h-2 rounded-full bg-fd-muted-foreground/15"
          style={{ width: `${width}%`, marginLeft: `${(index % 3) * 4}%` }}
        />
      ))}
    </div>
  );
}

/** What a tool shows while it works, then what it did. */
function Working({ children }: { children: ReactNode }) {
  return (
    <span className="tt-pick-working inline-flex items-center gap-1.5 text-fd-muted-foreground">
      <LoaderCircle className="size-3.5 animate-spin" aria-hidden />
      {children}
    </span>
  );
}

function Editor() {
  return (
    <Frame title="shop — vite.config.ts">
      <Code widths={[46, 72, 58, 34, 66, 52, 40]} />
      <div className="tt-mock-palette">
        <span className="flex items-center gap-2 border-b border-fd-border px-3 py-2 text-fd-muted-foreground">
          &gt; Teitunnel: share
        </span>
        <span className="flex items-center gap-2 bg-[var(--tt-accent-fill)] px-3 py-2 text-white">
          Teitunnel: Share This Workspace's Dev Server
        </span>
        <span className="px-3 py-2 text-fd-muted-foreground">Teitunnel: Share Port…</span>
      </div>
      <div className="tt-mock-toast tt-pick-result">
        <Logo className="size-4 shrink-0" />
        <span className="min-w-0">
          <span className="block truncate">Sharing localhost:5173 at</span>
          <span className="block truncate font-mono text-[var(--tt-accent-text)]">{address}</span>
        </span>
        <span className="tt-mock-button">Copy Address</span>
      </div>
    </Frame>
  );
}

function Raycast() {
  return (
    <div className="tt-mock tt-mock-raycast">
      <span className="flex items-center gap-2 border-b border-fd-border px-4 py-3 text-sm">
        <Search className="size-4 text-fd-muted-foreground" aria-hidden />
        share port 5173
      </span>
      <div className="flex flex-col gap-0.5 p-2">
        {[
          ["Share Port", "Share a local port at a public address and copy the address."],
          ["List Shares", "Your shares: copy an address, open it, open its requests or stop it."],
          ["Run Doctor", "Check routes, shares and the connector for problems."],
        ].map(([name, note], index) => (
          <span
            key={name}
            className={`flex items-center gap-3 rounded-lg px-3 py-2 ${index === 0 ? "bg-fd-accent" : ""}`}
          >
            <Logo className="size-4 shrink-0" />
            <span className="shrink-0 font-medium">{name}</span>
            <span className="truncate text-xs text-fd-muted-foreground">{note}</span>
          </span>
        ))}
      </div>
      <div className="flex justify-center p-3">
        <span className="tt-mock-hud tt-pick-result">
          <Check className="size-3.5 text-[var(--tt-live-text)]" aria-hidden /> Address copied
        </span>
      </div>
    </div>
  );
}

function Browser() {
  return (
    <Frame
      title={
        <span className="inline-flex items-center gap-1.5">
          <Globe className="size-3" aria-hidden /> localhost:3000/pricing
        </span>
      }
    >
      <Code widths={[30, 64, 48, 56]} />
      <div className="tt-mock-popup">
        <p className="flex items-center gap-2 font-medium">
          <Logo className="size-4" /> Teitunnel
        </p>
        <div className="tt-pick-before mt-2">
          <p className="text-xs text-fd-muted-foreground">This page runs on this computer.</p>
          <span className="tt-mock-button mt-3 w-full justify-center bg-fd-foreground text-fd-background">
            Share localhost:3000
          </span>
        </div>
        <div className="tt-pick-result mt-2">
          <p className="text-xs text-fd-muted-foreground">This page is shared at</p>
          <p className="mt-1 truncate font-mono text-xs text-[var(--tt-accent-text)]">{address}</p>
          <span className="tt-mock-button mt-3 w-full justify-center">
            <Copy className="size-3" aria-hidden /> Copy URL
          </span>
        </div>
      </div>
    </Frame>
  );
}

function Preview() {
  return (
    <Frame title="Add a pricing page #42">
      <div className="flex flex-col gap-3 p-4">
        <span className="flex items-center justify-between rounded-lg border border-fd-border px-3 py-2 text-xs">
          <span className="font-mono">teitunnel / preview</span>
          <span className="tt-pick-before">
            <Working>Publishing</Working>
          </span>
          <span className="tt-pick-result inline-flex items-center gap-1 text-[var(--tt-live-text)]">
            <Check className="size-3.5" aria-hidden /> Successful
          </span>
        </span>
        <div className="tt-pick-result rounded-lg border border-fd-border">
          <p className="border-b border-fd-border px-3 py-2 text-xs text-fd-muted-foreground">
            <span className="font-medium text-fd-foreground">github-actions</span> bot commented
          </p>
          <div className="space-y-1.5 px-3 py-3 text-[13px]">
            <p>
              <strong className="font-semibold">Preview:</strong>{" "}
              <span className="text-[var(--tt-accent-text)] underline underline-offset-2">
                https://pr-42.yourhost.com
              </span>
            </p>
            <p className="text-fd-muted-foreground">
              Updated for <code className="text-xs">3f9c2e1</code>.
            </p>
            <p className="text-[11px] text-fd-muted-foreground">
              Published with Teitunnel on your own Cloudflare account.
            </p>
          </div>
        </div>
      </div>
    </Frame>
  );
}

function Compose() {
  const yaml = [
    "services:",
    "  teitunnel:",
    "    image: teispace/teitunnel",
    "    environment:",
    "      CLOUDFLARE_API_TOKEN_FILE: /run/secrets/…",
    "    restart: unless-stopped",
    "  web:",
    "    image: nginx:alpine",
  ];
  return (
    <Frame title="compose.yaml">
      <pre className="overflow-hidden p-4 font-mono text-[12px] leading-relaxed">
        {yaml.map((line, index) => (
          <span
            key={line}
            className={`block ${index >= 1 && index <= 5 ? "text-fd-foreground" : "text-fd-muted-foreground"}`}
          >
            {line}
          </span>
        ))}
      </pre>
      <div className="mx-4 mb-4 rounded-lg border border-fd-border px-3 py-2 font-mono text-[12px]">
        <p className="text-fd-muted-foreground">$ docker compose up -d --wait</p>
        <p className="tt-pick-before">
          <Working>Starting teitunnel</Working>
        </p>
        <p className="tt-pick-result text-[var(--tt-live-text)]">
          ✔ Container shop-teitunnel-1 Healthy
        </p>
      </div>
    </Frame>
  );
}

function Shell() {
  return (
    <Frame title="zsh">
      <div className="flex flex-col gap-1 p-4 font-mono text-[12.5px] leading-relaxed">
        <p>
          <span className="text-fd-muted-foreground">$</span> brew install
          teispace/tap/teitunnel-cli
        </p>
        <p>
          <span className="text-fd-muted-foreground">$</span> teitunnel share 5173
        </p>
        <p className="text-fd-muted-foreground">Sharing localhost:5173…</p>
        <p className="tt-pick-before">
          <Working>Getting a URL</Working>
        </p>
        <p className="tt-pick-result text-[var(--tt-accent-text)]">https://{address}</p>
      </div>
    </Frame>
  );
}

export const toolDemos: PickerItem[] = [
  {
    id: "editors",
    icon: <Blocks />,
    name: "Editors",
    note: "VS Code, Cursor, Windsurf, JetBrains IDEs",
    panel: (
      <>
        <Editor />
        <GuideLink href="/docs/guides/integrations/">Editor extensions</GuideLink>
      </>
    ),
  },
  {
    id: "raycast",
    icon: <Command />,
    name: "Raycast",
    note: "share from anywhere",
    panel: (
      <>
        <Raycast />
        <GuideLink href="/docs/guides/integrations/">Raycast</GuideLink>
      </>
    ),
  },
  {
    id: "browser",
    icon: <Puzzle />,
    name: "Browser extension",
    note: "share the page you're on",
    panel: (
      <>
        <Browser />
        <GuideLink href="/docs/guides/browser-extension/">Browser extension</GuideLink>
      </>
    ),
  },
  {
    id: "ci",
    icon: <GitPullRequest />,
    name: "CI previews",
    note: "GitHub Action, GitLab CI",
    panel: (
      <>
        <Preview />
        <GuideLink href="/docs/guides/ci-previews/">CI previews</GuideLink>
      </>
    ),
  },
  {
    id: "docker",
    icon: <Container />,
    name: "Docker",
    note: "image and Compose",
    panel: (
      <>
        <Compose />
        <GuideLink href="/docs/guides/servers/">Servers and Docker</GuideLink>
      </>
    ),
  },
  {
    id: "terminal",
    icon: <SquareTerminal />,
    name: "Terminal",
    note: "Homebrew, scripts, JSON output",
    panel: (
      <>
        <Shell />
        <GuideLink href="/docs/reference/cli/">Command line</GuideLink>
      </>
    ),
  },
];
