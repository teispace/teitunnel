// Measures a packaged build: bundle size, cold start (process start → first frame, as
// the app logs it) and idle memory (the app plus the WebKit processes it started),
// each run in an isolated data directory so no real accounts or connectors load.
//
// Usage: node scripts/measure-app.ts [path/to/Teitunnel.app] [runs=3]
import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const app = resolve(process.argv[2] ?? "../../target/release/bundle/macos/Teitunnel.app");
const runs = Number(process.argv[3] ?? 3);
const binary = join(app, "Contents/MacOS/Teitunnel");
const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));
const median = (values: number[]) =>
  [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)] ?? 0;

const bundleKb = Number(execFileSync("du", ["-sk", app], { encoding: "utf8" }).split("\t")[0]);

/** RSS (KB) of `pid` plus WebKit helpers started within 3 s after `since` (ms). */
function memoryKb(pid: number, since: number): number {
  const rows = execFileSync("ps", ["-axo", "pid=,lstart=,rss=,comm="], { encoding: "utf8" })
    .split("\n")
    .map((line) => line.trim().match(/^(\d+)\s+(\w{3} \w{3}\s+\d+ [\d:]+ \d{4})\s+(\d+)\s+(.*)$/))
    .filter((m): m is RegExpMatchArray => m !== null);
  let total = 0;
  for (const [, id, started, rss, command] of rows) {
    const startedAt = Date.parse(started ?? "");
    const ours =
      Number(id) === pid ||
      ((command ?? "").includes("com.apple.WebKit") &&
        startedAt >= since - 1_000 &&
        startedAt <= since + 3_000);
    if (ours) total += Number(rss);
  }
  return total;
}

async function once() {
  const data = mkdtempSync(join(tmpdir(), "teitunnel-measure-"));
  const launched = Date.now();
  const child = spawn(binary, [], {
    env: { ...process.env, TEITUNNEL_DATA_DIR: data },
    stdio: "ignore",
  });
  try {
    let startupMs: number | null = null;
    for (let waited = 0; waited < 30_000 && startupMs === null; waited += 250) {
      await sleep(250);
      for (const file of safeList(join(data, "logs"))) {
        const match = readFileSync(join(data, "logs", file), "utf8").match(/startup_ms=(\d+)/);
        if (match) startupMs = Number(match[1]);
      }
    }
    if (startupMs === null) throw new Error("the app never reported its first frame");
    await sleep(20_000); // settle to idle
    return { startupMs, idleKb: memoryKb(child.pid ?? 0, launched) };
  } finally {
    child.kill("SIGTERM");
    await sleep(1_000);
    rmSync(data, { recursive: true, force: true });
  }
}

function safeList(dir: string): string[] {
  try {
    return readdirSync(dir);
  } catch {
    return [];
  }
}

const results = [];
for (let i = 0; i < runs; i++) results.push(await once());
process.stdout.write(
  `${JSON.stringify(
    {
      app,
      bundleMb: +(bundleKb / 1024).toFixed(1),
      runs,
      coldStartMs: {
        median: median(results.map((r) => r.startupMs)),
        all: results.map((r) => r.startupMs),
      },
      idleMemoryMb: {
        median: +(median(results.map((r) => r.idleKb)) / 1024).toFixed(0),
        all: results.map((r) => +(r.idleKb / 1024).toFixed(0)),
      },
    },
    null,
    2,
  )}\n`,
);
