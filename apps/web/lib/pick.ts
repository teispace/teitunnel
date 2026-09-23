import type { Platform } from "./detect";
import type { Download, Os } from "./release";

export interface Choice {
  /** What the main button downloads; null: it opens the download page instead. */
  download: Download | null;
  label: string;
  /** A quieter link for the other builds of the same system. */
  alternative: { label: string; href: string } | null;
}

const names: Record<Os, string> = { macos: "macOS", windows: "Windows", linux: "Linux" };

/** The page that starts a download and explains installing it. */
export function thanksHref(download: Download): string {
  return `/download/thanks/?file=${encodeURIComponent(download.name)}`;
}

/** The main download for the visitor's system (apps, not the CLI). */
export function choose(platform: Platform, downloads: readonly Download[]): Choice {
  const apps = downloads.filter((d) => d.kind !== "cli");
  const find = (os: Os, kinds: string[], arch?: string) =>
    kinds
      .map((kind) => apps.find((d) => d.os === os && d.kind === kind && (!arch || d.arch === arch)))
      .find(Boolean) ?? null;
  const fallback: Choice = { download: null, label: "Download", alternative: null };
  switch (platform.os) {
    case "macos": {
      const download = find("macos", ["dmg"]);
      return download
        ? {
            download,
            label: "Download for macOS",
            alternative: { label: "Windows and Linux", href: "/download/" },
          }
        : fallback;
    }
    case "windows": {
      const arm = platform.arch === "arm64";
      const download = find("windows", ["setup"], arm ? "arm64" : "x64");
      return download
        ? {
            download,
            label: arm ? "Download for Windows on Arm" : `Download for ${names.windows}`,
            alternative: {
              label: arm ? "Windows x64" : "Windows on Arm",
              href: "/download/#windows",
            },
          }
        : fallback;
    }
    case "linux": {
      // Ubuntu and Debian are the most common desktops; other formats are one click away.
      const download = find("linux", ["deb"], platform.arch ?? "x64");
      return download
        ? {
            download,
            label: `Download for ${names.linux} (.deb)`,
            alternative: { label: ".rpm, AppImage and Arm", href: "/download/#linux" },
          }
        : fallback;
    }
    default:
      return fallback;
  }
}
