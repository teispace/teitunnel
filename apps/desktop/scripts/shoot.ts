// Renders app routes in WebKit (light + dark) and saves PNGs, for visual review when the
// native window can't be captured (CI, locked screen). Native vibrancy is approximated
// by painting the sidebar with the measured material colour.
//
// Usage: node scripts/shoot.ts <out-dir> [route ...]   (default route: /dev/gallery)
// Env: SHOOT_SCROLL=<selector> scrolls it into view; SHOOT_ACTIONS=<sel;sel> clicks them;
// SHOOT_KEYS=<key;key> presses keys (Playwright names, e.g. Meta+k); SHOOT_SIZE=620x500.
// SHOOT_CONTRAST=more emulates Increase Contrast; SHOOT_REDUCED_MOTION=1 Reduce Motion.
// SHOOT_FILL=<selector=>value;…> types into fields before the actions run.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { webkit } from "@playwright/test";
import { createServer } from "vite";

const [outDir = "screenshots", ...routes] = process.argv.slice(2);
const targets = routes.length > 0 ? routes : ["/dev/gallery"];
const actions = (process.env["SHOOT_ACTIONS"] ?? "").split(";").filter(Boolean);
const scrollTo = process.env["SHOOT_SCROLL"];
const keys = (process.env["SHOOT_KEYS"] ?? "").split(";").filter(Boolean);
const fills = (process.env["SHOOT_FILL"] ?? "")
  .split(";")
  .filter(Boolean)
  .map((pair) => pair.split("=>") as [string, string]);
const [width = 1120, height = 720] = (process.env["SHOOT_SIZE"] ?? "")
  .split("x")
  .map(Number)
  .filter(Boolean);

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
      viewport: { width, height },
      deviceScaleFactor: 2,
      colorScheme: scheme,
      contrast: process.env["SHOOT_CONTRAST"] === "more" ? "more" : "no-preference",
      reducedMotion: process.env["SHOOT_REDUCED_MOTION"] ? "reduce" : "no-preference",
    });
    for (const route of targets) {
      await page.goto(new URL(route.replace(/^\//, ""), base).toString());
      await page.waitForSelector("h1");
      await page.addStyleTag({ content: VIBRANCY });
      await page.evaluate(() => {
        document.documentElement.dataset["windowActive"] = "true";
      });
      if (scrollTo) await page.locator(scrollTo).first().scrollIntoViewIfNeeded();
      for (const [selector, value] of fills) await page.fill(selector, value ?? "");
      for (const selector of actions) await page.click(selector);
      for (const key of keys) await page.keyboard.press(key);
      await page.waitForTimeout(700);
      const variant = process.env["SHOOT_CONTRAST"] === "more" ? "-contrast" : "";
      const name = `${route.replace(/\W+/g, "-").replace(/^-|-$/g, "") || "overview"}-${scheme}${variant}.png`;
      await page.screenshot({ path: join(outDir, name) });
      process.stdout.write(`${join(outDir, name)}\n`);
    }
    await page.close();
  }
} finally {
  await browser.close();
  await server.close();
}
