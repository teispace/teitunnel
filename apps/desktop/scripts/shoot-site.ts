// Renders the screenshots of the website (apps/web/public/screens) from the dev mocks, in
// WebKit, light and dark, and saves them as WebP. Same browser setup as shoot.ts.
//
// Usage: node scripts/shoot-site.ts [name ...]   (default: every shot below)
// Needs `cwebp` on the PATH (Homebrew: `brew install webp`).
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { type Page, webkit } from "@playwright/test";
import { createServer } from "vite";

interface Shot {
  name: string;
  route: string;
  /** CSS pixels; the image is twice as large. Default: the main window, 1280×800. */
  size?: [number, number];
  /** Steps, in order: `click:<selector>`, `fill:<selector>=><value>`, `scroll:<selector>`
   * (to the top), `key:<key>`, `wait:<ms>`. */
  steps?: string[];
}

const shots: Shot[] = [
  { name: "overview", route: "/" },
  { name: "routes", route: "/routes" },
  { name: "quick-share", route: "/quick-share" },
  {
    name: "inspector",
    route: "/inspector",
    steps: ['click:text="/api/projects?limit=20"'],
  },
  { name: "snapshots", route: "/snapshots" },
  { name: "comments", route: "/comments" },
  {
    name: "protection",
    route: "/routes",
    steps: ["click:text=Edit Protection…"],
  },
  { name: "local-domains", route: "/local-domains" },
  {
    name: "doctor",
    route: "/doctor",
    steps: ["click:text=docs.teispace.com has no DNS record"],
  },
  {
    name: "analytics",
    route: "/analytics",
    steps: ['click:role=radio[name="Week"]'],
  },
  { name: "activity", route: "/activity" },
  { name: "tunnels", route: "/tunnels" },
  { name: "domains", route: "/domains" },
  { name: "projects", route: "/projects" },
  {
    name: "new-route",
    route: "/routes?add=true",
    steps: [
      'fill:role=dialog >> input[placeholder^="Port"]=>3000',
      'fill:role=textbox[name="Subdomain"]=>shop',
    ],
  },
  {
    name: "review",
    route: "/routes?add=true",
    steps: [
      'fill:role=dialog >> input[placeholder^="Port"]=>3000',
      'fill:role=textbox[name="Subdomain"]=>shop',
      'click:role=button[name="Review"]',
    ],
  },
  {
    name: "accounts",
    route: "/settings",
    // The docs show it at this size (content/docs/getting-started/first-route.mdx).
    size: [620, 500],
    steps: ['click:role=tab[name="Accounts"]', "click:text=Permissions"],
  },
  {
    name: "integrations",
    route: "/settings",
    size: [620, 400],
    steps: ['click:role=tab[name="Integrations"]'],
  },
  {
    name: "offline-inbox",
    route: "/routes",
    steps: ['click:role=option >> text="app.teispace.com"', "scroll:text=Service Tokens"],
  },
  {
    name: "route-pause",
    route: "/routes",
    steps: ['click:role=option >> text="teispace.dev"', "scroll:text=Pause and Schedule"],
  },
  {
    name: "move-computer",
    route: "/settings",
    size: [620, 380],
    steps: ["scroll:text=Move to Another Computer"],
  },
  {
    name: "agents",
    route: "/settings?agents",
    size: [620, 620],
    steps: ["scroll:text=AI Tools"],
  },
];

// Measured NSVisualEffectView `sidebar` material on macOS 27, before our tint.
const VIBRANCY =
  "aside[aria-label=Sidebar]{background-color:light-dark(#e7e7e7,#454646)!important;background-image:linear-gradient(var(--surface-sidebar),var(--surface-sidebar))}";

const wanted = process.argv.slice(2);
const selected = wanted.length > 0 ? shots.filter((s) => wanted.includes(s.name)) : shots;
const outDir = resolve(import.meta.dirname, "../../web/public/screens");
const work = mkdtempSync(join(tmpdir(), "teitunnel-site-"));
mkdirSync(outDir, { recursive: true });

async function run(page: Page, steps: string[]) {
  for (const step of steps) {
    const at = step.indexOf(":");
    const kind = step.slice(0, at);
    const arg = step.slice(at + 1);
    if (kind === "click") await page.locator(arg).first().click();
    else if (kind === "fill") {
      const [selector = "", value = ""] = arg.split("=>");
      await page.locator(selector).first().fill(value);
    } else if (kind === "key") await page.keyboard.press(arg);
    else if (kind === "scroll")
      await page
        .locator(arg)
        .first()
        .evaluate((element) => {
          // Scroll the nearest scrolling pane, not the window (headers stay in place).
          let pane = element.parentElement;
          while (pane && !/(auto|scroll)/.test(getComputedStyle(pane).overflowY)) {
            pane = pane.parentElement;
          }
          if (!pane) return;
          const top = element.getBoundingClientRect().top - pane.getBoundingClientRect().top;
          pane.scrollTop += top - 12;
        });
    else if (kind === "wait") await page.waitForTimeout(Number(arg));
  }
}

const server = await createServer({ server: { port: 1431, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls?.local[0] ?? "http://localhost:1431/";
const browser = await webkit.launch();

try {
  for (const scheme of ["light", "dark"] as const) {
    for (const shot of selected) {
      const [width, height] = shot.size ?? [1280, 800];
      const page = await browser.newPage({
        viewport: { width, height },
        deviceScaleFactor: 2,
        colorScheme: scheme,
      });
      const url = new URL(shot.route.replace(/^\//, ""), base);
      url.searchParams.set("clean", "");
      await page.goto(url.toString());
      await page.waitForSelector("h1");
      await page.addStyleTag({ content: VIBRANCY });
      await page.evaluate(() => {
        document.documentElement.dataset["windowActive"] = "true";
      });
      await page.waitForTimeout(400);
      await run(page, shot.steps ?? []);
      await page.waitForTimeout(900);
      const png = join(work, `${shot.name}-${scheme}.png`);
      const webp = join(outDir, `${shot.name}-${scheme}.webp`);
      await page.screenshot({ path: png });
      execFileSync("cwebp", ["-quiet", "-q", "82", "-m", "6", "-sharp_yuv", png, "-o", webp]);
      process.stdout.write(`${webp}\n`);
      await page.close();
    }
  }
} finally {
  await browser.close();
  await server.close();
  rmSync(work, { recursive: true, force: true });
}
