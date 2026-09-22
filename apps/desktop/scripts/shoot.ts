// Renders app routes in WebKit (light + dark) and saves PNGs, for visual review when the
// native window can't be captured (CI, locked screen). Native vibrancy is approximated
// by painting the sidebar with the measured material colour.
//
// Usage: node scripts/shoot.ts <out-dir> [route ...]   (default route: /dev/gallery)
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { webkit } from "@playwright/test";
import { createServer } from "vite";

const [outDir = "screenshots", ...routes] = process.argv.slice(2);
const targets = routes.length > 0 ? routes : ["/dev/gallery"];
const actions = (process.env["SHOOT_ACTIONS"] ?? "").split(";").filter(Boolean);

// Measured NSVisualEffectView `sidebar` material on macOS 27 (D-024), before our tint.
const VIBRANCY =
  "aside[aria-label=Sidebar]{background-color:light-dark(#e7e7e7,#454646)!important;background-image:linear-gradient(var(--surface-sidebar),var(--surface-sidebar))}";

mkdirSync(outDir, { recursive: true });
const server = await createServer({ server: { port: 1430, strictPort: false }, logLevel: "error" });
await server.listen();
const base = server.resolvedUrls?.local[0] ?? "http://localhost:1430/";
const browser = await webkit.launch();

try {
  for (const scheme of ["light", "dark"] as const) {
    const page = await browser.newPage({
      viewport: { width: 1120, height: 720 },
      deviceScaleFactor: 2,
      colorScheme: scheme,
    });
    for (const route of targets) {
      await page.goto(new URL(route.replace(/^\//, ""), base).toString());
      await page.addStyleTag({ content: VIBRANCY });
      await page.evaluate(() => {
        document.documentElement.dataset["windowActive"] = "true";
      });
      for (const selector of actions) await page.click(selector);
      await page.waitForTimeout(700);
      const name = `${route.replace(/\W+/g, "-").replace(/^-|-$/g, "") || "overview"}-${scheme}.png`;
      await page.screenshot({ path: join(outDir, name) });
      process.stdout.write(`${join(outDir, name)}\n`);
    }
    await page.close();
  }
} finally {
  await browser.close();
  await server.close();
}
