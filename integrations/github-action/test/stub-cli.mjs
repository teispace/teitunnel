#!/usr/bin/env node
// A stand-in `teitunnel` for the action's flow tests: records each call (argv and the
// environment the action set) in $TEITUNNEL_DATA_DIR/calls.jsonl and answers like the
// real CLI does for the commands the action runs.

import { appendFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const args = process.argv.slice(2);
const data = process.env.TEITUNNEL_DATA_DIR;
appendFileSync(
  join(data, "calls.jsonl"),
  `${JSON.stringify({
    args,
    owner: process.env.TEITUNNEL_OWNER,
    machine: process.env.TEITUNNEL_MACHINE_NAME,
    token: process.env.CLOUDFLARE_API_TOKEN === "cf-secret",
    password: process.env.TEITUNNEL_SNAPSHOT_PASSWORD ?? null,
  })}\n`,
);
const on = args[args.indexOf("--on") + 1];

switch (`${args[0]} ${args[1] ?? ""}`.trim()) {
  case "cloudflared status":
    process.stdout.write("/usr/local/bin/cloudflared\t2026.9.0\n");
    break;
  case "snapshot publish":
    if (on.startsWith("held")) {
      process.stderr.write("teitunnel: held.example.com is reserved by alice@mac.\n");
      process.exit(3);
    }
    process.stdout.write(" 1. Upload 2 files\n");
    process.stdout.write(
      `${JSON.stringify({ url: `https://${on}`, hostname: on, name: args[4] })}\n`,
    );
    break;
  case "snapshot rm":
    process.stdout.write("No Snapshot; nothing to delete.\n");
    break;
  case "shares --json":
    process.stdout.write(`${JSON.stringify({ domains: [], terminals: [] })}\n`);
    break;
  case "tunnel delete":
    break;
  default:
    if (args[0] === "share") {
      if (on.startsWith("held")) {
        process.stderr.write(`teitunnel: ${on} is reserved by alice@mac.\n`);
        process.exit(3);
      }
      process.stdout.write(`${JSON.stringify({ url: `https://${on}`, hostname: on })}\n`);
      process.on("SIGTERM", () => {
        writeFileSync(join(data, "stopped"), "yes");
        process.exit(0);
      });
      setInterval(() => {}, 1000);
    } else {
      process.stderr.write(`unexpected: ${args.join(" ")}\n`);
      process.exit(2);
    }
}
