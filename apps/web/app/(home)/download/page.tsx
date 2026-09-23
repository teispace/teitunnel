import { ArrowUpRight, ShieldCheck } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";
import { DownloadButton } from "@/components/download-button";
import { FileRow } from "@/components/downloads";
import { type Download, formatDate, latestRelease, type Os } from "@/lib/release";
import { ogImage, pageMetadata } from "@/lib/seo";
import { site } from "@/lib/site";

export const metadata: Metadata = pageMetadata({
  title: "Download Teitunnel for macOS, Windows and Linux",
  description:
    "Download Teitunnel, the free Cloudflare Tunnel app, for macOS (Apple silicon and Intel), Windows (x64 and Arm) and Linux (.deb, .rpm, AppImage), or teitunnel-cli for servers.",
  path: "/download/",
  image: ogImage(["download"]),
});

function Platform({
  id,
  title,
  requirements,
  files,
  children,
}: {
  id: Os;
  title: string;
  requirements: string;
  files: Download[];
  children?: ReactNode;
}) {
  return (
    <section
      id={id}
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 scroll-mt-24 flex-col gap-4 rounded-2xl border border-fd-border bg-fd-card p-6"
    >
      <div>
        <h2 id={`${id}-title`} className="text-xl font-semibold tracking-tight">
          {title}
        </h2>
        <p className="mt-1 text-sm text-fd-muted-foreground">{requirements}</p>
      </div>
      {files.length > 0 ? (
        <ul className="flex flex-col gap-2">
          {files.map((download) => (
            <FileRow key={download.name} download={download} />
          ))}
        </ul>
      ) : null}
      {children ? <div className="text-sm text-fd-muted-foreground">{children}</div> : null}
    </section>
  );
}

export default async function DownloadPage() {
  const release = await latestRelease();
  const downloads = release?.downloads ?? [];
  const of = (os: Os, cli = false) =>
    downloads.filter((d) => d.os === os && (d.kind === "cli") === cli);
  const cli = downloads.filter((d) => d.kind === "cli");

  return (
    <main className="mx-auto w-full max-w-6xl px-6 pt-16 pb-24 md:pt-24">
      <header className="mb-14 flex max-w-3xl flex-col gap-5">
        <p className="font-mono text-xs uppercase tracking-[0.18em] text-fd-muted-foreground">
          {release ? `Beta · version ${release.version} · ${formatDate(release.date)}` : "Beta"}
        </p>
        <h1 className="text-4xl font-semibold tracking-tight text-balance md:text-6xl">
          Download Teitunnel
        </h1>
        {release ? (
          <>
            <p className="text-lg text-fd-muted-foreground">
              Free and open source, for macOS, Windows and Linux. It updates itself, and fetches and
              verifies cloudflared for you.
            </p>
            <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
              <DownloadButton downloads={downloads} showAlternative={false} />
              <a
                href={release.notesUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1 text-sm text-fd-muted-foreground hover:text-fd-foreground"
              >
                What's new in {release.version} <ArrowUpRight className="size-3.5" aria-hidden />
              </a>
            </div>
          </>
        ) : (
          <p className="text-lg text-fd-muted-foreground">
            The first public beta is on its way. Watch{" "}
            <a className="underline underline-offset-4" href={site.github}>
              the repository
            </a>{" "}
            to hear when it's out, or build it from source today.
          </p>
        )}
      </header>

      <div className="grid gap-6 md:grid-cols-3">
        <Platform
          id="macos"
          title="macOS"
          requirements="macOS 14 Sonoma or later. One download for Apple silicon and Intel."
          files={of("macos")}
        >
          Signed with Teispace's Developer ID and notarized by Apple.
        </Platform>
        <Platform
          id="windows"
          title="Windows"
          requirements="Windows 10 or 11, x64 or Arm. Installs for your user, no administrator needed."
          files={of("windows")}
        >
          This beta isn't code-signed yet, so Windows may say it protected your PC: choose{" "}
          <strong className="font-medium text-fd-foreground">More info ▸ Run anyway</strong>.{" "}
          <Link className="underline underline-offset-4" href="/code-signing/">
            Why
          </Link>
        </Platform>
        <Platform
          id="linux"
          title="Linux"
          requirements="x64 or Arm64 with WebKitGTK 4.1: Ubuntu 22.04, Debian 12, Fedora 40 or later."
          files={of("linux")}
        >
          The .deb and .rpm install with your package manager; the AppImage runs anywhere.
        </Platform>
      </div>

      <section
        id="cli"
        aria-labelledby="cli-title"
        className="mt-6 grid scroll-mt-24 gap-6 rounded-2xl border border-fd-border bg-fd-card p-6 md:grid-cols-[1fr_2fr]"
      >
        <div className="min-w-0">
          <h2 id="cli-title" className="text-xl font-semibold tracking-tight">
            Command line and servers
          </h2>
          <p className="mt-1 text-sm text-fd-muted-foreground">
            <code>teitunnel-cli</code> runs routes on any machine without a desktop: a VPS, a cloud
            VM or a container, with a web dashboard. See{" "}
            <Link className="underline underline-offset-4" href="/docs/guides/servers">
              Servers and containers
            </Link>
            .
          </p>
        </div>
        {cli.length > 0 ? (
          <ul className="grid min-w-0 gap-2 sm:grid-cols-2">
            {cli.map((download) => (
              <FileRow key={download.name} download={download} />
            ))}
          </ul>
        ) : null}
      </section>

      <section className="mt-14 grid gap-6 md:grid-cols-2">
        <div className="flex gap-4">
          <ShieldCheck className="mt-0.5 size-5 shrink-0" aria-hidden />
          <div className="text-sm text-fd-muted-foreground">
            <h2 className="mb-1 text-base font-semibold text-fd-foreground">
              Verify your download
            </h2>
            Every file is listed in{" "}
            {release?.checksumsUrl ? (
              <a className="underline underline-offset-4" href={release.checksumsUrl}>
                SHA256SUMS.txt
              </a>
            ) : (
              "SHA256SUMS.txt"
            )}{" "}
            and carries GitHub build provenance, so you can check it was built from this repository.{" "}
            <Link className="underline underline-offset-4" href="/docs/reference/verify">
              How to verify
            </Link>
          </div>
        </div>
        <div className="text-sm text-fd-muted-foreground">
          <h2 className="mb-1 text-base font-semibold text-fd-foreground">Your privacy</h2>
          Teitunnel has no telemetry. It talks to Cloudflare to do what you ask, and to GitHub to
          fetch cloudflared and check for updates.{" "}
          <Link className="underline underline-offset-4" href="/privacy/">
            Privacy
          </Link>{" "}
          ·{" "}
          <Link className="underline underline-offset-4" href="/code-signing/">
            Code signing policy
          </Link>
        </div>
      </section>
    </main>
  );
}
