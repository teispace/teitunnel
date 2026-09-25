// The action end to end with a stub CLI and a stand-in GitHub API: outputs, state for
// the post step, the share's lifetime, the comment kept to one, and refusals.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const stub = join(here, "stub-cli.mjs");
const skip = process.platform === "win32" ? "the stub CLI is a script with a shebang" : false;

let server;
let api;
const comments = [];

before(async () => {
  if (skip) return;
  chmodSync(stub, 0o755);
  server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", () => {
      res.setHeader("content-type", "application/json");
      if (req.method === "GET") return res.end(JSON.stringify(comments));
      const text = JSON.parse(body).body;
      if (req.method === "POST") comments.push({ id: comments.length + 1, body: text });
      else comments.find((c) => c.id === Number(req.url.split("/").pop())).body = text;
      res.statusCode = req.method === "POST" ? 201 : 200;
      res.end("{}");
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  api = `http://127.0.0.1:${server.address().port}`;
});

after(() => server?.close());

/** Runs one step of the action as the runner does; resolves with its outputs and state. */
function step(script, inputs, work, state = {}) {
  const outputs = join(work, `output-${script}`);
  const saved = join(work, `state-${script}`);
  writeFileSync(outputs, "");
  writeFileSync(saved, "");
  const env = {
    PATH: process.env.PATH,
    RUNNER_TEMP: work,
    GITHUB_OUTPUT: outputs,
    GITHUB_STATE: saved,
    GITHUB_API_URL: api,
    GITHUB_REPOSITORY: "acme/web",
    GITHUB_RUN_ID: "77",
    GITHUB_RUN_ATTEMPT: "1",
    GITHUB_SHA: "0123456789abcdef",
    GITHUB_EVENT_PATH: join(work, "event.json"),
    ...Object.fromEntries(Object.entries(inputs).map(([k, v]) => [`INPUT_${k.toUpperCase()}`, v])),
    ...Object.fromEntries(Object.entries(state).map(([k, v]) => [`STATE_${k}`, v])),
  };
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [join(here, "..", "src", script)], { env });
    let log = "";
    child.stdout.on("data", (c) => {
      log += c;
    });
    child.stderr.on("data", (c) => {
      log += c;
    });
    child.on("close", (code) => {
      const read = (file) =>
        Object.fromEntries(
          readFileSync(file, "utf8")
            .split("\n")
            .filter(Boolean)
            .map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]),
        );
      resolve({ code, log, outputs: read(outputs), state: read(saved) });
    });
  });
}

function workspace() {
  const work = mkdtempSync(join(tmpdir(), "teitunnel-action-"));
  writeFileSync(
    join(work, "event.json"),
    JSON.stringify({ pull_request: { number: 7, head: { ref: "feat/x", sha: "abcdef0123" } } }),
  );
  return work;
}

const calls = (work) =>
  readFileSync(join(work, "teitunnel-data", "calls.jsonl"), "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));

const base = {
  "cloudflare-api-token": "cf-secret",
  zone: "teispace.com",
  "cli-path": stub,
  "github-token": "gh-token",
};

describe("the action", { skip }, () => {
  it("publishes a Snapshot, comments once, and updates that comment on the next push", async () => {
    const work = workspace();
    const first = await step("main.mjs", { ...base, mode: "snapshot", password: "hunter22" }, work);
    assert.equal(first.code, 0, first.log);
    assert.deepEqual(first.outputs, {
      hostname: "pr-7-preview.teispace.com",
      url: "https://pr-7-preview.teispace.com",
    });
    assert.ok(first.log.includes("::add-mask::cf-secret"));
    assert.ok(first.log.includes("::add-mask::hunter22"));
    const [publish] = calls(work);
    assert.deepEqual(publish.args.slice(0, 5), ["snapshot", "publish", ".", "--name", "web-pr-7"]);
    assert.ok(publish.args.includes("--or-update"));
    assert.equal(publish.token, true, "the token comes by environment");
    assert.equal(publish.password, "hunter22", "so does the password");
    assert.ok(!publish.args.includes("cf-secret") && !publish.args.includes("hunter22"));
    assert.equal(publish.owner, "github-actions/acme/web");
    assert.equal(comments.length, 1);
    assert.match(comments[0].body, /https:\/\/pr-7\.preview\.example\.com/);

    const second = await step("main.mjs", { ...base, mode: "snapshot" }, work);
    assert.equal(second.code, 0, second.log);
    assert.equal(comments.length, 1, "the same comment, edited");
    // Snapshots stay up: the post step leaves them.
    const post = await step("post.mjs", base, work, second.state);
    assert.equal(post.code, 0, post.log);
    assert.equal(calls(work).length, 2);

    const cleanup = await step("main.mjs", { ...base, mode: "cleanup" }, work);
    assert.equal(cleanup.code, 0, cleanup.log);
    assert.deepEqual(calls(work).at(-1).args, [
      "snapshot",
      "rm",
      "pr-7-preview.teispace.com",
      "--missing-ok",
      "--yes",
    ]);
    assert.equal(comments.length, 1);
    assert.match(comments[0].body, /Removed/);
  });

  it("keeps a share up for the job and stops it, its route and its tunnel in the post step", async () => {
    comments.length = 0;
    const work = workspace();
    const inputs = { ...base, mode: "share", port: "3000", hostname: "{branch}.{zone}" };
    const main = await step("main.mjs", inputs, work);
    assert.equal(main.code, 0, main.log);
    assert.equal(main.outputs.url, "https://feat-x.teispace.com");
    assert.equal(main.state.machine, "gh-web-77-1");
    assert.ok(Number(main.state.pid) > 0);
    assert.ok(!existsSync(join(work, "teitunnel-data", "stopped")), "still running");

    const post = await step("post.mjs", inputs, work, main.state);
    assert.equal(post.code, 0, post.log);
    assert.ok(existsSync(join(work, "teitunnel-data", "stopped")), "stopped by a signal");
    assert.deepEqual(calls(work).at(-1).args, ["tunnel", "delete", "gh-web-77-1", "--yes"]);
    assert.match(comments[0].body, /stopped/);
  });

  it("fails clearly when someone else holds the hostname", async () => {
    const work = workspace();
    for (const mode of ["snapshot", "share"]) {
      const held = await step(
        "main.mjs",
        { ...base, mode, port: "3000", hostname: "held-{number}.{zone}", comment: "false" },
        work,
      );
      assert.equal(held.code, 1, held.log);
      assert.match(held.log, /::error::.*reserved by alice@mac/);
    }
  });
});
