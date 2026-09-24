import type { Platform } from "./detect";
import type { Arch, Download, Kind, Os } from "./release";

/** A tab of the download page: a system, or the command line. */
export type PanelId = Os | "cli";

/** One row of other downloads: a format, with a link per architecture. */
export interface Variant {
  label: string;
  hint: string;
  files: { label: string; download: Download }[];
}

/** A command that installs this panel's software, with where it applies. */
export interface InstallCommand {
  label: string;
  command: string;
}

export interface Panel {
  id: PanelId;
  title: string;
  requirement: string;
  /** The file the big button downloads; null before a release. */
  primary: Download | null;
  /** "Universal · .dmg", shown under the button. */
  primaryDetail: string;
  variants: Variant[];
  /** Commands that install it from a terminal; none before a release. */
  commands: InstallCommand[];
}

export const panelIds: PanelId[] = ["macos", "windows", "linux", "cli"];

export const panelNames: Record<PanelId, string> = {
  macos: "macOS",
  windows: "Windows",
  linux: "Linux",
  cli: "Command line",
};

const archLabels: Record<Arch, string> = { universal: "Universal", x64: "x64", arm64: "Arm64" };

const formats: Record<Kind, { label: string; hint: string }> = {
  dmg: { label: ".dmg", hint: "Apple silicon and Intel" },
  setup: { label: "Installer", hint: "Windows 10 and 11" },
  deb: { label: ".deb", hint: "Ubuntu, Debian, Mint" },
  rpm: { label: ".rpm", hint: "Fedora, RHEL, openSUSE" },
  appimage: { label: "AppImage", hint: "Any distribution" },
  cli: { label: "teitunnel-cli", hint: "" },
};

const archOrder: Arch[] = ["universal", "x64", "arm64"];

/** Package managers and images that follow the releases (D-084). */
const channels = {
  cask: "brew install --cask teispace/tap/teitunnel",
  formula: "brew install teispace/tap/teitunnel-cli",
  docker: "docker run -d -e CLOUDFLARE_API_TOKEN -v teitunnel:/data ghcr.io/teispace/teitunnel",
};

function byArch(a: Download, b: Download): number {
  return archOrder.indexOf(a.arch) - archOrder.indexOf(b.arch);
}

function variant(files: Download[], label: string, hint: string): Variant | null {
  if (files.length === 0) return null;
  return {
    label,
    hint,
    files: [...files].sort(byArch).map((download) => ({
      label: archLabels[download.arch],
      download,
    })),
  };
}

/** The file for the visitor's CPU when known, x64 otherwise, else whatever there is. */
function preferred(files: Download[], arch: Platform["arch"]): Download | null {
  return (
    files.find((d) => d.arch === (arch ?? "x64")) ??
    files.find((d) => d.arch === "universal") ??
    files[0] ??
    null
  );
}

/** Everything the download page shows, for the visitor's platform. */
export function panels(downloads: readonly Download[], platform: Platform): Panel[] {
  const of = (os: Os, kind: Kind) => downloads.filter((d) => d.os === os && d.kind === kind);
  const cli = downloads.filter((d) => d.kind === "cli");
  const detail = (d: Download | null) =>
    d ? `${archLabels[d.arch]} · ${formats[d.kind].label}` : "";

  const dmg = preferred(of("macos", "dmg"), null);
  const setup = preferred(of("windows", "setup"), platform.os === "windows" ? platform.arch : null);
  const linuxArch = platform.os === "linux" ? platform.arch : null;
  const deb = preferred(of("linux", "deb"), linuxArch);
  const cliLinux = preferred(
    cli.filter((d) => d.os === "linux"),
    linuxArch,
  );

  const linuxVariants = (["deb", "rpm", "appimage"] as const)
    .map((kind) => variant(of("linux", kind), formats[kind].label, formats[kind].hint))
    .filter((v): v is Variant => v !== null);
  const cliVariants = (["macos", "windows", "linux"] as const)
    .map((os) =>
      variant(
        cli.filter((d) => d.os === os),
        panelNames[os],
        os === "linux" ? ".tar.gz" : ".zip",
      ),
    )
    .filter((v): v is Variant => v !== null);

  return [
    {
      id: "macos",
      title: "macOS",
      requirement: "macOS 14 Sonoma or later · Apple silicon and Intel",
      primary: dmg,
      primaryDetail: detail(dmg),
      variants: [],
      commands: dmg ? [{ label: "Homebrew", command: channels.cask }] : [],
    },
    {
      id: "windows",
      title: "Windows",
      requirement: "Windows 10 or 11 · installs for your user, no administrator",
      primary: setup,
      primaryDetail: detail(setup),
      variants: [variant(of("windows", "setup"), "Installer", "Windows 10 and 11")].filter(
        (v): v is Variant => v !== null,
      ),
      commands: [],
    },
    {
      id: "linux",
      title: "Linux",
      requirement: "WebKitGTK 4.1 · Ubuntu 22.04, Debian 12, Fedora 40 or later",
      primary: deb,
      primaryDetail: deb ? `${detail(deb)} · ${formats.deb.hint}` : "",
      variants: linuxVariants,
      commands: deb
        ? [{ label: "Ubuntu, Debian, Mint", command: `sudo apt install ./${deb.name}` }]
        : [],
    },
    {
      id: "cli",
      title: "Command line",
      requirement: "teitunnel-cli for servers, VMs and containers",
      primary: cliLinux,
      primaryDetail: cliLinux ? `Linux · ${archLabels[cliLinux.arch]} · .tar.gz` : "",
      variants: cliVariants,
      commands: cliLinux
        ? [
            {
              label: "Linux",
              command: `curl -L ${cliLinux.url} | sudo tar -xz -C /usr/local/bin teitunnel-cli`,
            },
            { label: "Homebrew, on macOS or Linux", command: channels.formula },
            { label: "Docker", command: channels.docker },
          ]
        : [],
    },
  ];
}

/** The tab to open first: the one in the address (`#linux`), else the visitor's system. */
export function initialPanel(hash: string, platform: Platform): PanelId {
  const fromHash = hash.replace(/^#/, "") as PanelId;
  if (panelIds.includes(fromHash)) return fromHash;
  return platform.os === "windows" || platform.os === "linux" ? platform.os : "macos";
}
