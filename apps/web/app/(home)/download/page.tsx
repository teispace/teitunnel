import { ArrowUpRight, BadgeCheck, EyeOff, RefreshCw } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import { DownloadChooser } from "@/components/download-chooser";
import { FileRow } from "@/components/downloads";
import { delay } from "@/components/landing";
import { OsIcon } from "@/components/os-icons";
import { panelIds, panelNames } from "@/lib/platforms";
import { formatDate, latestRelease } from "@/lib/release";
import { ogImage, pageMetadata } from "@/lib/seo";
import { site } from "@/lib/site";

export const metadata: Metadata = pageMetadata({
  title: "Download Teitunnel for macOS, Windows and Linux",
  description:
    "Download Teitunnel, the free Cloudflare Tunnel app, for macOS (Apple silicon and Intel), Windows (x64 and Arm) and Linux (.deb, .rpm, AppImage), or teitunnel-cli for servers.",
  path: "/download/",
  image: ogImage(["download"]),
});

const promises = [
  {
    icon: BadgeCheck,
    title: "Verified builds",
    body: "Built by GitHub Actions from this repository, with checksums and build provenance.",
    href: "/docs/reference/verify/",
    link: "How to verify",
  },
  {
    icon: RefreshCw,
    title: "Updates itself",
    body: "Signed updates in the background, installed when you restart. Nothing to re-download.",
    href: "/docs/getting-started/install/#updates",
    link: "About updates",
  },
  {
    icon: EyeOff,
    title: "No telemetry",
    body: "It talks to Cloudflare to do what you ask, and to GitHub for cloudflared and updates.",
    href: "/privacy/",
    link: "Privacy",
  },
];

export default async function DownloadPage() {
  const release = await latestRelease();
  const downloads = release?.downloads ?? [];

  return (
    <main className="mx-auto w-full max-w-5xl px-6 pt-16 pb-24 md:pt-24">
      <header className="mx-auto mb-12 flex max-w-2xl flex-col items-center gap-5 text-center">
        <p
          data-hero
          className="font-mono text-xs uppercase tracking-[0.18em] text-fd-muted-foreground"
        >
          {release ? `Beta ${release.version} · ${formatDate(release.date)}` : "Beta"}
        </p>
        <h1
          data-hero
          style={delay(80)}
          className="text-4xl font-semibold tracking-tight text-balance md:text-6xl"
        >
          Download Teitunnel
        </h1>
        <p data-hero style={delay(160)} className="text-lg text-balance text-fd-muted-foreground">
          Free and open source, for your computer and your servers. It installs and verifies
          cloudflared for you.
        </p>
        {release ? (
          <a
            data-hero
            style={delay(220)}
            href={release.notesUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 text-sm text-fd-muted-foreground transition-colors hover:text-fd-foreground"
          >
            What's new in {release.version} <ArrowUpRight className="size-3.5" aria-hidden />
          </a>
        ) : null}
      </header>

      {release ? (
        <div data-hero style={delay(280)}>
          <DownloadChooser downloads={downloads} />
        </div>
      ) : (
        <div className="mx-auto max-w-xl rounded-3xl border border-fd-border bg-fd-card p-8 text-center">
          <p className="text-lg font-medium">The first public beta is on its way.</p>
          <p className="mt-2 text-fd-muted-foreground">
            Watch{" "}
            <a className="underline underline-offset-4" href={site.github}>
              the repository
            </a>{" "}
            to hear when it's out, or build it from source today.
          </p>
        </div>
      )}

      <section className="mt-16 grid gap-8 sm:grid-cols-3">
        {promises.map(({ icon: Icon, title, body, href, link }, index) => (
          <div key={title} data-reveal style={delay(index * 80)} className="flex flex-col gap-2">
            <Icon className="size-5 text-[var(--tt-accent)]" aria-hidden />
            <h2 className="font-medium">{title}</h2>
            <p className="text-sm text-fd-muted-foreground">{body}</p>
            <Link href={href} className="text-sm font-medium underline-offset-4 hover:underline">
              {link}
            </Link>
          </div>
        ))}
      </section>

      <p className="mt-10 text-center text-sm text-fd-muted-foreground">
        <Link href="/code-signing/" className="underline underline-offset-4">
          Code signing policy
        </Link>{" "}
        ·{" "}
        <Link href="/privacy/" className="underline underline-offset-4">
          Privacy
        </Link>{" "}
        · Windows code signing by SignPath Foundation (pending)
      </p>

      {release ? (
        <details
          id="all"
          className="group mt-16 rounded-2xl border border-fd-border [&_summary::-webkit-details-marker]:hidden"
        >
          <summary className="flex cursor-pointer list-none items-center justify-between gap-4 px-6 py-4 text-sm font-medium">
            Every file in {release.version}
            <span className="text-xl leading-none text-fd-muted-foreground transition-transform duration-300 group-open:rotate-45">
              +
            </span>
          </summary>
          <div className="flex flex-col gap-6 border-t border-fd-border px-6 py-6">
            {panelIds.map((id) => {
              const files = downloads.filter((d) =>
                id === "cli" ? d.kind === "cli" : d.os === id && d.kind !== "cli",
              );
              if (files.length === 0) return null;
              return (
                <div key={id} className="grid gap-3 md:grid-cols-[10rem_1fr]">
                  <p className="flex items-center gap-2 text-sm font-medium">
                    <OsIcon os={id} className="size-4" /> {panelNames[id]}
                  </p>
                  <ul className="grid min-w-0 gap-2 sm:grid-cols-2">
                    {files.map((download) => (
                      <FileRow key={download.name} download={download} />
                    ))}
                  </ul>
                </div>
              );
            })}
            <p className="text-sm text-fd-muted-foreground">
              Checksums:{" "}
              {release.checksumsUrl ? (
                <a className="underline underline-offset-4" href={release.checksumsUrl}>
                  SHA256SUMS.txt
                </a>
              ) : (
                "SHA256SUMS.txt"
              )}{" "}
              ·{" "}
              <Link className="underline underline-offset-4" href="/code-signing/">
                Code signing policy
              </Link>{" "}
              ·{" "}
              <a
                className="underline underline-offset-4"
                href={`${site.github}/blob/main/CONTRIBUTING.md`}
              >
                Build from source
              </a>
            </p>
          </div>
        </details>
      ) : null}
    </main>
  );
}
