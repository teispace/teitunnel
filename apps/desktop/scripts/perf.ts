// Measures the M5 exit criteria on the dev stress page (src/dev/stress.tsx) in WebKit:
// a log at 2,000 lines/s and a 3,600-point chart at 20 Hz, rendered together. With
// `inspector`, the Inspector list instead (src/dev/stress-inspector.tsx): 10,000
// requests, 200/s more arriving, filtered, scrolled every frame, measured on a
// production build (`vite build --mode perf`), since React's development checks
// dominate a list this busy.
// Reports frame-interval percentiles, long frames and DOM size over time.
//
// Usage: node scripts/perf.ts [seconds=30] [inspector]

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { webkit } from "@playwright/test";
import { build, createServer, preview } from "vite";

/** What the stress page records (src/dev/stress.tsx). */
interface Stats {
  frames: number[];
  logLines: number;
  chartUpdates: number;
}
type StressWindow = Window & { __stress?: Stats };

const seconds = Number(process.argv[2] ?? 30);
const inspector = process.argv.includes("inspector");
const outDir = inspector ? mkdtempSync(join(tmpdir(), "teitunnel-perf-")) : null;
let server: { resolvedUrls: { local: string[] } | null; close(): Promise<void> };
if (outDir) {
  await build({ mode: "perf", logLevel: "error", build: { outDir, emptyOutDir: true } });
  server = await preview({ build: { outDir }, preview: { port: 1432 }, logLevel: "error" });
} else {
  const dev = await createServer({ server: { port: 1431, strictPort: false }, logLevel: "error" });
  await dev.listen();
  server = dev;
}
const base = server.resolvedUrls?.local[0] ?? "http://localhost:1431/";
const browser = await webkit.launch();

const percentile = (values: number[], p: number) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))] ?? 0;
};

try {
  const page = await browser.newPage({ viewport: { width: 1120, height: 900 } });
  await page.goto(new URL(inspector ? "dev/stress-inspector" : "dev/stress", base).toString());
  await page.waitForSelector("h1");
  // Let the page warm up (JIT, first layout), then measure from a clean slate.
  await page.waitForTimeout(2_000);
  await page.evaluate(() => {
    const stats = (window as StressWindow).__stress;
    if (stats) stats.frames.length = 0;
  });
  const nodes: number[] = [];
  for (let s = 0; s < seconds; s++) {
    await page.waitForTimeout(1_000);
    nodes.push(await page.evaluate(() => document.getElementsByTagName("*").length));
  }
  const stats = await page.evaluate(() => (window as StressWindow).__stress);
  if (!stats) throw new Error("the stress page didn't start");
  const frames = stats.frames;
  const result = {
    seconds,
    frames: frames.length,
    fps: Math.round(frames.length / seconds),
    p50: percentile(frames, 50).toFixed(1),
    p95: percentile(frames, 95).toFixed(1),
    p99: percentile(frames, 99).toFixed(1),
    max: Math.max(...frames).toFixed(1),
    longFrames: frames.filter((f) => f > 50).length,
    [inspector ? "requestsPerSecond" : "logLinesPerSecond"]: Math.round(
      stats.logLines / (seconds + 2),
    ),
    chartUpdates: stats.chartUpdates,
    domNodes: { first: nodes[0], last: nodes.at(-1), max: Math.max(...nodes) },
  };
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
} finally {
  await browser.close();
  await server.close();
  if (outDir) rmSync(outDir, { recursive: true, force: true });
}
