import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { fromUserAgent } from "./detect.ts";
import { choose, thanksHref } from "./pick.ts";
import type { Download } from "./release.ts";

const file = (
  name: string,
  os: Download["os"],
  arch: Download["arch"],
  kind: Download["kind"],
) => ({
  name,
  url: `https://github.com/teispace/teitunnel/releases/download/v0.1.0/${name}`,
  size: 1,
  os,
  arch,
  kind,
});
const downloads: Download[] = [
  file("Teitunnel_0.1.0_universal.dmg", "macos", "universal", "dmg"),
  file("Teitunnel_0.1.0_x64-setup.exe", "windows", "x64", "setup"),
  file("Teitunnel_0.1.0_arm64-setup.exe", "windows", "arm64", "setup"),
  file("Teitunnel_0.1.0_amd64.deb", "linux", "x64", "deb"),
  file("Teitunnel_0.1.0_arm64.deb", "linux", "arm64", "deb"),
  file("teitunnel-cli_0.1.0_linux-x64.tar.gz", "linux", "x64", "cli"),
];

const ua = {
  safari:
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/19.0 Safari/605.1.15",
  edge: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/140.0 Safari/537.36 Edg/140.0",
  linuxArm: "Mozilla/5.0 (X11; Linux aarch64; rv:140.0) Gecko/20100101 Firefox/140.0",
  iphone:
    "Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148",
};

describe("download choice", () => {
  it("gives every Mac the universal disk image", () => {
    const choice = choose(fromUserAgent(ua.safari), downloads);
    assert.equal(choice.download?.name, "Teitunnel_0.1.0_universal.dmg");
    assert.equal(choice.label, "Download for macOS");
  });

  it("picks the Windows build for the CPU the browser reports", () => {
    assert.equal(choose(fromUserAgent(ua.edge), downloads).download?.arch, "x64");
    const arm = choose({ os: "windows", arch: "arm64" }, downloads);
    assert.equal(arm.download?.name, "Teitunnel_0.1.0_arm64-setup.exe");
    assert.equal(arm.alternative?.label, "Windows x64");
  });

  it("offers Linux the .deb for its architecture, never the CLI", () => {
    const choice = choose(fromUserAgent(ua.linuxArm), downloads);
    assert.equal(choice.download?.name, "Teitunnel_0.1.0_arm64.deb");
    assert.equal(choose({ os: "linux", arch: null }, downloads).download?.kind, "deb");
  });

  it("sends phones and unknown systems, or a missing release, to the download page", () => {
    assert.equal(choose(fromUserAgent(ua.iphone), downloads).download, null);
    assert.equal(choose({ os: null, arch: null }, downloads).download, null);
    assert.equal(choose(fromUserAgent(ua.safari), []).download, null);
  });

  it("links the thanks page by file name only", () => {
    assert.equal(
      thanksHref(downloads[0] as Download),
      "/download/thanks/?file=Teitunnel_0.1.0_universal.dmg",
    );
  });
});
