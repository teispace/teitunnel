import { Download as DownloadIcon } from "lucide-react";
import Link from "next/link";
import { thanksHref } from "@/lib/pick";
import { type Download, formatSize, latestRelease, type Os } from "@/lib/release";
import { DownloadButton } from "./download-button";

const archNames = { universal: "Apple silicon and Intel", x64: "x64", arm64: "Arm64" } as const;
const kindNames = {
  dmg: "Disk image (.dmg)",
  setup: "Installer (.exe)",
  deb: "Ubuntu, Debian (.deb)",
  rpm: "Fedora, openSUSE (.rpm)",
  appimage: "Any distribution (AppImage)",
  cli: "Command line",
} as const;

/** One file of the release, linking to the page that downloads it and explains the install. */
export function FileRow({ download }: { download: Download }) {
  return (
    <li className="min-w-0 list-none">
      <Link
        href={thanksHref(download)}
        className="group flex items-center justify-between gap-4 rounded-lg border border-fd-border bg-fd-background px-4 py-3 no-underline transition-colors hover:bg-fd-accent"
      >
        <span className="min-w-0">
          <span className="block text-sm font-medium text-fd-foreground">
            {kindNames[download.kind]} · {archNames[download.arch]}
          </span>
          <span className="block truncate font-mono text-xs text-fd-muted-foreground">
            {download.name} · {formatSize(download.size)}
          </span>
        </span>
        <DownloadIcon
          className="size-4 shrink-0 text-fd-muted-foreground transition-transform group-hover:translate-y-0.5 group-hover:text-fd-foreground"
          aria-hidden
        />
      </Link>
    </li>
  );
}

/**
 * Downloads inside the docs: the reader's own download first, or one system's files
 * (`os`), or the command line (`cli`). Falls back to the download page before a release.
 */
export async function Downloads({ os, cli = false }: { os?: Os; cli?: boolean }) {
  const release = await latestRelease();
  const downloads = release?.downloads ?? [];
  const files = downloads.filter(
    (d) => (d.kind === "cli") === cli && (os === undefined || d.os === os),
  );
  return (
    <div className="not-prose my-6 flex flex-col gap-4 rounded-xl border border-fd-border bg-fd-card p-4">
      {os === undefined && !cli ? (
        <div className="flex flex-wrap items-center justify-between gap-3">
          <DownloadButton downloads={downloads} />
          <span className="text-xs text-fd-muted-foreground">
            {release ? `Version ${release.version}` : "Latest beta"} · free
          </span>
        </div>
      ) : null}
      {(os !== undefined || cli) && files.length > 0 ? (
        <ul className="grid gap-2 p-0 sm:grid-cols-2">
          {files.map((download) => (
            <FileRow key={download.name} download={download} />
          ))}
        </ul>
      ) : null}
      {files.length === 0 ? (
        <p className="text-sm text-fd-muted-foreground">
          The files are on the{" "}
          <Link href="/download/" className="underline underline-offset-4">
            download page
          </Link>
          .
        </p>
      ) : null}
    </div>
  );
}
