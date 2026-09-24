// Accessibility audit: runs axe-core (WCAG 2.2 A/AA rules) on every screen in WebKit,
// light and dark, with the dev IPC mock, and lists violations. Exits 1 if any.
//
// Contrast policy (D-052): the default appearance uses macOS's own label colours, which
// sit below 4.5:1 for secondary text by design, so colour contrast is checked with
// Increase Contrast on, where every rule must pass. All other rules apply in both.
//
// Usage: node scripts/a11y.ts [route ...]
import AxeBuilder from "@axe-core/playwright";
import { webkit } from "@playwright/test";
import { createServer } from "vite";

const ROUTES = [
  "/",
  "/routes",
  "/quick-share",
  "/inspector",
  "/domains",
  "/tunnels",
  "/activity",
  "/doctor",
  "/settings",
];
const routes = process.argv.slice(2).length > 0 ? process.argv.slice(2) : ROUTES;

const server = await createServer({ server: { port: 1432, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls?.local[0] ?? "http://localhost:1432/";
const browser = await webkit.launch();
let total = 0;

// The sidebar's native material, as the screenshot tool paints it (D-024), so contrast
// is measured against what's really behind the text.
const VIBRANCY =
  "aside[aria-label=Sidebar]{background-color:light-dark(#e7e7e7,#454646)!important;background-image:linear-gradient(var(--surface-sidebar),var(--surface-sidebar))}";

try {
  const modes = [
    { scheme: "light", contrast: "no-preference" },
    { scheme: "dark", contrast: "no-preference" },
    { scheme: "light", contrast: "more" },
    { scheme: "dark", contrast: "more" },
  ] as const;
  for (const { scheme, contrast } of modes) {
    // axe needs an explicit context (it injects into every frame).
    const context = await browser.newContext({
      viewport: { width: 1120, height: 800 },
      colorScheme: scheme,
      contrast,
    });
    const label = contrast === "more" ? `${scheme}+contrast` : scheme;
    const page = await context.newPage();
    for (const route of routes) {
      await page.goto(new URL(route.replace(/^\//, ""), base).toString());
      await page.waitForSelector("h1");
      await page.addStyleTag({ content: VIBRANCY });
      await page.waitForTimeout(500);
      const axe = new AxeBuilder({ page }).withTags([
        "wcag2a",
        "wcag2aa",
        "wcag21a",
        "wcag21aa",
        "wcag22aa",
      ]);
      if (contrast !== "more") axe.disableRules(["color-contrast"]);
      const { violations } = await axe.analyze();
      for (const v of violations) {
        total += 1;
        process.stdout.write(`${label} ${route}  [${v.impact}] ${v.id}: ${v.help}\n`);
        for (const node of v.nodes.slice(0, 3)) {
          process.stdout.write(
            `    ${node.target.join(" ")}  ${node.failureSummary?.split("\n")[1]?.trim() ?? ""}\n`,
          );
        }
      }
    }
    await context.close();
  }
} finally {
  await browser.close();
  await server.close();
}
process.stdout.write(total === 0 ? "No violations.\n" : `${total} violation(s).\n`);
process.exit(total === 0 ? 0 : 1);
