import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { initialPanel, panels } from "./platforms.ts";
import type { Download } from "./release.ts";

const file = (
  name: string,
  os: Download["os"],
  arch: Download["arch"],
  kind: Download["kind"],
) => ({
  name,
  url: `https://example.com/${name}`,
  size: 1,
  os,
  arch,
  kind,
});

const release: Download[] = [
  file("T_universal.dmg", "macos", "universal", "dmg"),
  file("T_arm64-setup.exe", "windows", "arm64", "setup"),
  file("T_x64-setup.exe", "windows", "x64", "setup"),
  file("T_amd64.deb", "linux", "x64", "deb"),
  file("T_arm64.deb", "linux", "arm64", "deb"),
  file("T.x86_64.rpm", "linux", "x64", "rpm"),
  file("T_amd64.AppImage", "linux", "x64", "appimage"),
  file("cli_linux-x64.tar.gz", "linux", "x64", "cli"),
  file("cli_linux-arm64.tar.gz", "linux", "arm64", "cli"),
  file("cli_macos-universal.zip", "macos", "universal", "cli"),
];

const get = (list: ReturnType<typeof panels>, id: string) => {
  const panel = list.find((p) => p.id === id);
  assert.ok(panel);
  return panel;
};

describe("download panels", () => {
  it("picks the visitor's architecture, and x64 when it's unknown", () => {
    const unknown = panels(release, { os: "macos", arch: null });
    assert.equal(get(unknown, "windows").primary?.name, "T_x64-setup.exe");
    assert.equal(get(unknown, "linux").primary?.name, "T_amd64.deb");
    const arm = panels(release, { os: "windows", arch: "arm64" });
    assert.equal(get(arm, "windows").primary?.name, "T_arm64-setup.exe");
    const linuxArm = panels(release, { os: "linux", arch: "arm64" });
    assert.equal(get(linuxArm, "linux").primary?.name, "T_arm64.deb");
    assert.equal(get(linuxArm, "cli").primary?.name, "cli_linux-arm64.tar.gz");
  });

  it("groups other downloads by format, architectures in a fixed order", () => {
    const linux = get(panels(release, { os: null, arch: null }), "linux");
    assert.deepEqual(
      linux.variants.map((v) => [v.label, v.files.map((f) => f.label)]),
      [
        [".deb", ["x64", "Arm64"]],
        [".rpm", ["x64"]],
        ["AppImage", ["x64"]],
      ],
    );
    const windows = get(panels(release, { os: null, arch: null }), "windows");
    assert.deepEqual(
      windows.variants[0]?.files.map((f) => f.label),
      ["x64", "Arm64"],
    );
  });

  it("gives install commands for the exact files", () => {
    const list = panels(release, { os: "linux", arch: "x64" });
    const linux = get(list, "linux").commands.map((c) => c.command);
    assert.match(linux[0] ?? "", /linux\/deb stable main.*sudo apt install teitunnel$/);
    assert.match(linux[1] ?? "", /teitunnel\.repo && sudo dnf install teitunnel$/);
    assert.equal(linux[2], "sudo apt install ./T_amd64.deb");
    const cli = get(list, "cli").commands.map((c) => c.command);
    assert.match(cli[0] ?? "", /cli_linux-x64\.tar\.gz \| sudo tar -xz/);
    assert.ok(cli.includes("brew install teispace/tap/teitunnel-cli"));
    assert.ok(cli.some((c) => c.endsWith("ghcr.io/teispace/teitunnel")));
    assert.deepEqual(
      get(list, "macos").commands.map((c) => c.command),
      ["brew install --cask teispace/tap/teitunnel"],
    );
  });

  it("has no files and no commands before a release", () => {
    for (const panel of panels([], { os: "macos", arch: null })) {
      assert.equal(panel.primary, null);
      assert.deepEqual(panel.commands, []);
      assert.deepEqual(panel.variants, []);
    }
  });

  it("opens the tab in the address, else the visitor's system", () => {
    assert.equal(initialPanel("#cli", { os: "macos", arch: null }), "cli");
    assert.equal(initialPanel("#nope", { os: "linux", arch: null }), "linux");
    assert.equal(initialPanel("", { os: "windows", arch: "x64" }), "windows");
    assert.equal(initialPanel("", { os: "mobile", arch: null }), "macos");
  });
});
