import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { describe, it } from "node:test";
import {
  assetFor,
  commentBody,
  label,
  lastJson,
  machineName,
  marker,
  parseSums,
  readInputs,
  renderHostname,
  shareArgs,
  snapshotArgs,
  snapshotName,
  templateContext,
  upsertComment,
  verifyChecksum,
} from "../src/lib.mjs";

const env = (inputs) =>
  Object.fromEntries(Object.entries(inputs).map(([k, v]) => [`INPUT_${k.toUpperCase()}`, v]));

describe("inputs", () => {
  it("reads defaults and refuses what can't work", () => {
    const inputs = readInputs(env({ "cloudflare-api-token": "t", zone: "teispace.com" }));
    assert.equal(inputs.mode, "snapshot");
    assert.equal(inputs.hostname, "pr-{number}-preview.{zone}");
    assert.equal(inputs.comment, true);
    assert.deepEqual(inputs.allow, []);
    assert.throws(() => readInputs(env({})), /cloudflare-api-token/);
    assert.throws(() => readInputs(env({ "cloudflare-api-token": "t", mode: "x" })), /mode/);
    assert.throws(() => readInputs(env({ "cloudflare-api-token": "t", mode: "share" })), /port/);
    assert.throws(() => readInputs(env({ "cloudflare-api-token": "t", build: "maybe" })), /build/);
    const allow = readInputs(
      env({ "cloudflare-api-token": "t", allow: "a@teispace.com, @teispace.dev\nb@teispace.app" }),
    );
    assert.deepEqual(allow.allow, ["a@teispace.com", "@teispace.dev", "b@teispace.app"]);
  });
});

describe("hostname templates", () => {
  const event = {
    pull_request: { number: 42, head: { ref: "feat/New Login!", sha: "abcdef1234567" } },
  };
  const context = templateContext({ GITHUB_REPOSITORY: "Teispace/Web.App" }, event, "Teispace.com");

  it("fills placeholders as DNS labels", () => {
    assert.equal(
      renderHostname("pr-{number}-preview.{zone}", context),
      "pr-42-preview.teispace.com",
    );
    assert.equal(
      renderHostname("{branch}.{repo}.{zone}", context),
      "feat-new-login.web-app.teispace.com",
    );
    assert.equal(renderHostname("{sha}.{owner}.{zone}", context), "abcdef1.teispace.teispace.com");
  });

  it("refuses templates that can't make a hostname", () => {
    assert.throws(() => renderHostname("pr-{number}.{zone}", { ...context, zone: "" }), /zone/);
    assert.throws(
      () => renderHostname("pr-{number}.{zone}", { ...context, number: "" }),
      /pull request/,
    );
    assert.throws(() => renderHostname("{nope}.{zone}", context), /Unknown placeholder/);
    assert.throws(() => renderHostname("-bad-.{zone}", context), /valid hostname/);
    assert.throws(() => renderHostname("single", context), /valid hostname/);
  });

  it("keeps labels within 63 characters", () => {
    const long = { ...context, branch: "x".repeat(100) };
    const hostname = renderHostname("{branch}.{zone}", long);
    assert.equal(hostname.split(".")[0].length, 63);
    assert.equal(label("--A__b--"), "a-b");
  });

  it("names Snapshots per repository and PR", () => {
    assert.equal(snapshotName("{repo}-pr-{number}", context), "web-app-pr-42");
    assert.equal(snapshotName("x".repeat(80), context).length, 40);
    assert.throws(() => snapshotName("{missing}", context), /empty/);
  });
});

describe("the released CLI", () => {
  it("picks this runner's archive", () => {
    assert.equal(assetFor("linux", "x64", "v0.3.0").name, "teitunnel-cli_0.3.0_linux-x64.tar.gz");
    assert.equal(
      assetFor("linux", "arm64", "0.3.0").name,
      "teitunnel-cli_0.3.0_linux-arm64.tar.gz",
    );
    assert.equal(
      assetFor("darwin", "arm64", "0.3.0").name,
      "teitunnel-cli_0.3.0_macos-universal.zip",
    );
    assert.equal(assetFor("win32", "x64", "0.3.0").binary, "teitunnel.exe");
    assert.throws(() => assetFor("linux", "ia32", "0.3.0"), /No Teitunnel CLI/);
    assert.throws(() => assetFor("freebsd", "x64", "0.3.0"), /No Teitunnel CLI/);
  });

  it("verifies the archive against SHA256SUMS.txt", () => {
    const bytes = Buffer.from("the archive");
    const hash = createHash("sha256").update(bytes).digest("hex");
    const sums = parseSums(
      `${hash}  teitunnel-cli_0.3.0_linux-x64.tar.gz\n${"0".repeat(64)} *other.zip\n`,
    );
    assert.equal(sums.size, 2);
    verifyChecksum(bytes, "teitunnel-cli_0.3.0_linux-x64.tar.gz", sums);
    assert.throws(
      () => verifyChecksum(Buffer.from("tampered"), "teitunnel-cli_0.3.0_linux-x64.tar.gz", sums),
      /doesn't match/,
    );
    assert.throws(() => verifyChecksum(bytes, "missing.zip", sums), /doesn't list/);
  });
});

describe("CLI arguments and output", () => {
  const inputs = readInputs(
    env({
      "cloudflare-api-token": "secret-token",
      port: "3000",
      mode: "share",
      expires: "2h",
      allow: "a@teispace.com",
      account: "Acme",
      password: "hunter22",
      build: "true",
    }),
  );

  it("never puts secrets in argv", () => {
    const share = shareArgs(inputs, "pr-1.teispace.com");
    assert.deepEqual(share, [
      "share",
      "3000",
      "--on",
      "pr-1.teispace.com",
      "--json",
      "--no-qr",
      "--account",
      "Acme",
      "--for",
      "2h",
      "--allow",
      "a@teispace.com",
    ]);
    const snapshot = snapshotArgs(inputs, "pr-1.teispace.com", "web-pr-1");
    assert.ok(snapshot.includes("--or-update") && snapshot.includes("--build"));
    assert.ok(snapshot.includes("--password"));
    for (const args of [share, snapshot]) {
      assert.ok(!args.join(" ").includes("secret-token"));
      assert.ok(!args.join(" ").includes("hunter22"));
    }
  });

  it("finds the JSON the CLI prints after its plan", () => {
    const out =
      ' 1. Upload 3 files\n    done: Upload\n{"url":"https://a.teispace.com","hostname":"a.teispace.com"}\n';
    assert.equal(lastJson(out)?.url, "https://a.teispace.com");
    assert.equal(lastJson("nothing here\n{broken"), null);
  });

  it("names the job's tunnel after the run", () => {
    assert.equal(
      machineName({ GITHUB_REPOSITORY: "acme/web", GITHUB_RUN_ID: "99", GITHUB_RUN_ATTEMPT: "2" }),
      "gh-web-99-2",
    );
  });
});

describe("the pull request comment", () => {
  function fakeGitHub(initial = []) {
    const comments = [...initial];
    const requests = [];
    const fetch = async (url, init = {}) => {
      const method = init.method ?? "GET";
      requests.push(`${method} ${url.replace("https://api.github.com", "")}`);
      const reply = (status, body) => ({ ok: status < 300, status, json: async () => body });
      if (method === "GET") {
        const page = Number(new URL(url).searchParams.get("page"));
        return reply(200, comments.slice((page - 1) * 100, page * 100));
      }
      const body = JSON.parse(init.body).body;
      if (method === "POST") {
        comments.push({ id: comments.length + 1, body });
        return reply(201, {});
      }
      const id = Number(url.split("/").pop());
      comments.find((c) => c.id === id).body = body;
      return reply(200, {});
    };
    return { comments, requests, fetch };
  }
  const args = (fetch, body) => ({
    fetch,
    apiUrl: "https://api.github.com",
    repository: "acme/web",
    number: 7,
    token: "gh",
    hostname: "pr-7.teispace.com",
    body,
  });

  it("is posted once and then edited in place", async () => {
    const github = fakeGitHub([{ id: 1, body: "unrelated" }]);
    const live = commentBody({
      hostname: "pr-7.teispace.com",
      url: "https://pr-7.teispace.com",
      state: "live",
      sha: "abc1234",
      mode: "snapshot",
    });
    assert.ok(live.startsWith(marker("pr-7.teispace.com")));
    assert.equal(await upsertComment(args(github.fetch, live)), "created");
    const again = commentBody({
      hostname: "pr-7.teispace.com",
      url: "https://pr-7.teispace.com",
      state: "live",
      sha: "def5678",
      mode: "snapshot",
    });
    assert.equal(await upsertComment(args(github.fetch, again)), "updated");
    assert.equal(github.comments.length, 2, "never a second preview comment");
    assert.match(github.comments[1].body, /def5678/);
    const removed = commentBody({
      hostname: "pr-7.teispace.com",
      url: "",
      state: "removed",
      mode: "snapshot",
    });
    await upsertComment(args(github.fetch, removed));
    assert.match(github.comments[1].body, /Removed/);
  });

  it("finds its comment on a later page", async () => {
    const many = Array.from({ length: 150 }, (_, i) => ({ id: i + 1, body: `c${i}` }));
    many[120].body = `${marker("pr-7.teispace.com")}\nold`;
    const github = fakeGitHub(many);
    assert.equal(await upsertComment(args(github.fetch, "new")), "updated");
    assert.equal(github.comments[120].body, "new");
    assert.ok(github.requests.includes("PATCH /repos/acme/web/issues/comments/121"));
  });

  it("reports a refused write (a fork's read-only token)", async () => {
    const fetch = async (_url, init = {}) =>
      init.method ? { ok: false, status: 403 } : { ok: true, status: 200, json: async () => [] };
    await assert.rejects(upsertComment(args(fetch, "x")), /HTTP 403/);
  });
});
