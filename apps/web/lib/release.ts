import { site } from "./site";

/** Operating systems with a download. */
export type Os = "macos" | "windows" | "linux";

export type Arch = "universal" | "x64" | "arm64";

export type Kind = "dmg" | "setup" | "deb" | "rpm" | "appimage" | "cli";

/** A downloadable file of a release. */
export interface Download {
  name: string;
  url: string;
  /** Bytes. */
  size: number;
  os: Os;
  arch: Arch;
  kind: Kind;
}

export interface Release {
  version: string;
  /** ISO date it was published. */
  date: string;
  notesUrl: string;
  checksumsUrl: string | null;
  downloads: Download[];
}

/** File-name patterns of release assets (see .github/workflows/release.yml). */
const patterns: [RegExp, Os, Arch, Kind][] = [
  [/_universal\.dmg$/, "macos", "universal", "dmg"],
  [/_x64-setup\.exe$/, "windows", "x64", "setup"],
  [/_arm64-setup\.exe$/, "windows", "arm64", "setup"],
  [/_amd64\.deb$/, "linux", "x64", "deb"],
  [/_arm64\.deb$/, "linux", "arm64", "deb"],
  [/\.x86_64\.rpm$/, "linux", "x64", "rpm"],
  [/\.aarch64\.rpm$/, "linux", "arm64", "rpm"],
  [/_amd64\.AppImage$/, "linux", "x64", "appimage"],
  [/_aarch64\.AppImage$/, "linux", "arm64", "appimage"],
  [/^teitunnel-cli_.+_macos-universal\.zip$/, "macos", "universal", "cli"],
  [/^teitunnel-cli_.+_windows-x64\.zip$/, "windows", "x64", "cli"],
  [/^teitunnel-cli_.+_windows-arm64\.zip$/, "windows", "arm64", "cli"],
  [/^teitunnel-cli_.+_linux-x64\.tar\.gz$/, "linux", "x64", "cli"],
  [/^teitunnel-cli_.+_linux-arm64\.tar\.gz$/, "linux", "arm64", "cli"],
];

interface GitHubRelease {
  tag_name: string;
  published_at: string;
  html_url: string;
  assets: { name: string; size: number; browser_download_url: string }[];
}

/** Turns GitHub's release into the site's view of it (only files it knows). */
export function toRelease(release: GitHubRelease): Release {
  const downloads: Download[] = [];
  for (const asset of release.assets) {
    const match = patterns.find(([pattern]) => pattern.test(asset.name));
    if (!match) continue;
    const [, os, arch, kind] = match;
    downloads.push({
      name: asset.name,
      url: asset.browser_download_url,
      size: asset.size,
      os,
      arch,
      kind,
    });
  }
  const sums = release.assets.find((a) => a.name === "SHA256SUMS.txt");
  return {
    version: release.tag_name.replace(/^v/, ""),
    date: release.published_at,
    notesUrl: release.html_url,
    checksumsUrl: sums?.browser_download_url ?? null,
    downloads,
  };
}

/** Sample data for previewing the download pages before a release exists (never in CI). */
function preview(): Release {
  const v = "0.1.0";
  const names = [
    `Teitunnel_${v}_universal.dmg`,
    `Teitunnel_${v}_x64-setup.exe`,
    `Teitunnel_${v}_arm64-setup.exe`,
    `Teitunnel_${v}_amd64.deb`,
    `Teitunnel_${v}_arm64.deb`,
    `Teitunnel-${v}-1.x86_64.rpm`,
    `Teitunnel-${v}-1.aarch64.rpm`,
    `Teitunnel_${v}_amd64.AppImage`,
    `Teitunnel_${v}_aarch64.AppImage`,
    `teitunnel-cli_${v}_macos-universal.zip`,
    `teitunnel-cli_${v}_windows-x64.zip`,
    `teitunnel-cli_${v}_windows-arm64.zip`,
    `teitunnel-cli_${v}_linux-x64.tar.gz`,
    `teitunnel-cli_${v}_linux-arm64.tar.gz`,
    "SHA256SUMS.txt",
  ];
  return toRelease({
    tag_name: `v${v}`,
    published_at: "2026-09-24T12:00:00Z",
    html_url: `${site.github}/releases/tag/v${v}`,
    assets: names.map((name, i) => ({
      name,
      size: 9_000_000 + i * 1_300_000,
      browser_download_url: `${site.github}/releases/download/v${v}/${name}`,
    })),
  });
}

async function fetchLatest(): Promise<Release | null> {
  if (process.env.PREVIEW_RELEASE === "1" && !process.env.CI) return preview();
  const api = `https://api.github.com/repos/${site.repo}/releases/latest`;
  const headers: Record<string, string> = {
    accept: "application/vnd.github+json",
    "user-agent": "teitunnel-website",
  };
  if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  let lastError: unknown = null;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const response = await fetch(api, { headers, signal: AbortSignal.timeout(10_000) });
      // No release yet: the site says the beta is on its way.
      if (response.status === 404) return null;
      if (!response.ok) throw new Error(`GitHub answered ${response.status}`);
      return toRelease((await response.json()) as GitHubRelease);
    } catch (error) {
      lastError = error;
    }
  }
  // After a release the site must link to it: fail the build rather than publish a page
  // without downloads. Local and preview builds carry on without them.
  if (process.env.REQUIRE_RELEASE === "true") throw lastError;
  console.warn(`latest release unavailable, building without downloads: ${String(lastError)}`);
  return null;
}

let cached: Promise<Release | null> | null = null;

/** The latest published release, read once per build. */
export function latestRelease(): Promise<Release | null> {
  cached ??= fetchLatest();
  return cached;
}

/** "14.2 MB". */
export function formatSize(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

/** "September 23, 2026". */
export function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-US", { dateStyle: "long", timeZone: "UTC" });
}
