import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";

const root = resolve(import.meta.dirname, "..");

test("builds plain JavaScript for Chromium and Firefox", () => {
  const out = mkdtempSync(join(tmpdir(), "teitunnel-extension-"));
  execFileSync(process.execPath, [join(root, "scripts/build.ts"), out], { stdio: "pipe" });
  const chrome = JSON.parse(readFileSync(join(out, "chrome/manifest.json"), "utf8"));
  const firefox = JSON.parse(readFileSync(join(out, "firefox/manifest.json"), "utf8"));
  assert.deepEqual(chrome.permissions, ["nativeMessaging", "activeTab"]);
  assert.ok(chrome.key, "the key fixes the extension's id");
  assert.equal(firefox.key, undefined);
  assert.equal(firefox.browser_specific_settings.gecko.id, "browser@teitunnel.teispace.com");
  const popup = readFileSync(join(out, "chrome/popup.js"), "utf8");
  assert.match(popup, /from "\.\/host\.js"/);
  assert.doesNotMatch(popup, /: HTMLButtonElement|<T extends/);
  assert.ok(existsSync(join(out, "firefox/icons/128.png")));
});
