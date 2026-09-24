"use client";

import { ArrowRight, Check, Copy, Download as DownloadIcon } from "lucide-react";
import Link from "next/link";
import { type KeyboardEvent, useEffect, useState } from "react";
import { detectPlatform, type Platform } from "@/lib/detect";
import { thanksHref } from "@/lib/pick";
import {
  initialPanel,
  type Panel,
  type PanelId,
  panelIds,
  panelNames,
  panels,
} from "@/lib/platforms";
import { type Download, formatSize } from "@/lib/release";
import { OsIcon } from "./os-icons";

function CopyCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard.writeText(command).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    });
  };
  return (
    <div className="flex min-w-0 items-center gap-2 rounded-xl border border-fd-border bg-fd-background py-1.5 ps-4 pe-1.5">
      <code className="min-w-0 flex-1 truncate font-mono text-[13px]" title={command}>
        <span className="select-none text-fd-muted-foreground">$ </span>
        {command}
      </code>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? "Copied" : "Copy command"}
        className="flex size-8 shrink-0 items-center justify-center rounded-lg text-fd-muted-foreground transition-colors hover:bg-fd-accent hover:text-fd-foreground"
      >
        {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
      </button>
    </div>
  );
}

function Chip({ label, download }: { label: string; download: Download }) {
  return (
    <Link
      href={thanksHref(download)}
      title={`${download.name} · ${formatSize(download.size)}`}
      className="inline-flex h-8 items-center gap-1.5 rounded-full border border-fd-border bg-fd-background px-3 text-sm font-medium transition-colors hover:border-fd-foreground/30 hover:bg-fd-accent"
    >
      <DownloadIcon className="size-3.5 text-fd-muted-foreground" aria-hidden />
      {label}
    </Link>
  );
}

const installSteps: Record<PanelId, string[]> = {
  macos: [
    "Open the .dmg from your Downloads folder.",
    "Drag Teitunnel into Applications.",
    "Open it. macOS asks once whether to open an app from the internet: choose Open.",
  ],
  windows: [
    "Run the installer. It installs for your user, no administrator needed.",
    "If Windows says it protected your PC, choose More info, then Run anyway.",
  ],
  linux: [],
  cli: [],
};

const guides: Record<PanelId, [string, string]> = {
  macos: ["Installation guide", "/docs/getting-started/install/"],
  windows: ["Installation guide", "/docs/getting-started/install/"],
  linux: ["Installation guide", "/docs/getting-started/install/"],
  cli: ["Servers and containers", "/docs/guides/servers/"],
};

function Note({ id }: { id: PanelId }) {
  if (id === "cli")
    return (
      <p className="text-sm text-fd-muted-foreground">
        Also in every desktop download, and as a Docker image.
      </p>
    );
  if (id === "linux")
    return (
      <p className="text-sm text-fd-muted-foreground">
        The .deb and .rpm install with your package manager; the AppImage runs anywhere.
      </p>
    );
  return null;
}

function PanelView({ panel }: { panel: Panel }) {
  const [guide, guideHref] = guides[panel.id];
  const steps = installSteps[panel.id];
  return (
    <div
      id={`${panel.id}-panel`}
      role="tabpanel"
      aria-labelledby={`${panel.id}-tab`}
      className="tt-panel grid gap-10 rounded-3xl border border-fd-border bg-fd-card p-6 md:grid-cols-[1fr_1.15fr] md:p-10"
    >
      <div className="flex min-w-0 flex-col items-start gap-5">
        <OsIcon os={panel.id} className="size-10" />
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">{panel.title}</h2>
          <p className="mt-1 text-sm text-fd-muted-foreground">{panel.requirement}</p>
        </div>
        {panel.primary ? (
          <div className="flex flex-col items-start gap-2">
            <Link
              href={thanksHref(panel.primary)}
              className="inline-flex h-12 items-center gap-2.5 rounded-full bg-fd-foreground px-7 font-medium text-fd-background transition-[opacity,transform] duration-200 hover:opacity-85 active:scale-[0.98]"
            >
              <DownloadIcon className="size-4.5" aria-hidden />
              {panel.id === "cli" ? "Download teitunnel-cli" : `Download for ${panel.title}`}
            </Link>
            <p className="ps-1 font-mono text-xs text-fd-muted-foreground">
              {panel.primaryDetail} · {formatSize(panel.primary.size)}
            </p>
          </div>
        ) : null}
        <Note id={panel.id} />
        <Link
          href={guideHref}
          className="mt-auto inline-flex items-center gap-1 text-sm font-medium underline-offset-4 hover:underline"
        >
          {guide} <ArrowRight className="size-4" aria-hidden />
        </Link>
      </div>

      <div className="flex min-w-0 flex-col gap-8">
        {panel.variants.length > 0 ? (
          <section aria-label="Other downloads">
            <h3 className="mb-3 text-xs font-medium uppercase tracking-[0.14em] text-fd-muted-foreground">
              All {panel.id === "cli" ? "systems" : "formats"}
            </h3>
            <ul className="divide-y divide-fd-border rounded-2xl border border-fd-border bg-fd-background/50">
              {panel.variants.map((variant) => (
                <li
                  key={variant.label}
                  className="flex flex-wrap items-center justify-between gap-3 px-4 py-3"
                >
                  <span className="min-w-0">
                    <span className="block text-sm font-medium">{variant.label}</span>
                    <span className="block text-xs text-fd-muted-foreground">{variant.hint}</span>
                  </span>
                  <span className="flex flex-wrap gap-2">
                    {variant.files.map((file) => (
                      <Chip key={file.download.name} label={file.label} download={file.download} />
                    ))}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
        {steps.length > 0 ? (
          <section aria-label="How to install">
            <h3 className="mb-3 text-xs font-medium uppercase tracking-[0.14em] text-fd-muted-foreground">
              Install
            </h3>
            <ol className="flex flex-col gap-3">
              {steps.map((step, index) => (
                <li key={step} className="flex gap-3 text-sm">
                  <span className="flex size-6 shrink-0 items-center justify-center rounded-full border border-fd-border font-mono text-xs">
                    {index + 1}
                  </span>
                  <span className="pt-0.5 text-fd-muted-foreground">{step}</span>
                </li>
              ))}
            </ol>
          </section>
        ) : null}
        {panel.commands.length > 0 ? (
          <section aria-label="From a terminal">
            <h3 className="mb-3 text-xs font-medium uppercase tracking-[0.14em] text-fd-muted-foreground">
              {panel.id === "cli" ? "From a terminal" : "Or from a terminal"}
            </h3>
            <div className="space-y-3">
              {panel.commands.map(({ label, command }) => (
                <div key={label}>
                  <p className="mb-1.5 text-xs text-fd-muted-foreground">{label}</p>
                  <CopyCommand command={command} />
                </div>
              ))}
            </div>
          </section>
        ) : null}
      </div>
    </div>
  );
}

/**
 * The download page's platform tabs: the visitor's system first (or the one in the
 * address, `/download/#linux`), with its main file, every other format and an install
 * command. The server renders macOS; the browser switches to the right one.
 */
export function DownloadChooser({ downloads }: { downloads: Download[] }) {
  const unknown: Platform = { os: null, arch: null };
  const [platform, setPlatform] = useState<Platform>(unknown);
  const [selected, setSelected] = useState<PanelId>("macos");

  useEffect(() => {
    let live = true;
    void detectPlatform().then((detected) => {
      if (!live) return;
      setPlatform(detected);
      setSelected(initialPanel(window.location.hash, detected));
    });
    const onHash = () => {
      const id = window.location.hash.slice(1) as PanelId;
      if (panelIds.includes(id)) setSelected(id);
    };
    window.addEventListener("hashchange", onHash);
    return () => {
      live = false;
      window.removeEventListener("hashchange", onHash);
    };
  }, []);

  const all = panels(downloads, platform);
  const panel = all.find((p) => p.id === selected) ?? all[0];

  const select = (id: PanelId) => {
    setSelected(id);
    window.history.replaceState(null, "", `#${id}`);
  };
  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = panelIds[(panelIds.indexOf(selected) + step + panelIds.length) % panelIds.length];
    if (!next) return;
    select(next);
    document.getElementById(`${next}-tab`)?.focus();
  };

  return (
    <div className="flex flex-col gap-4">
      <div
        role="tablist"
        aria-label="Systems"
        onKeyDown={onKey}
        className="grid grid-cols-2 gap-3 sm:grid-cols-4"
      >
        {panelIds.map((id) => {
          const active = id === selected;
          return (
            <button
              key={id}
              id={`${id}-tab`}
              type="button"
              role="tab"
              aria-selected={active}
              aria-controls={`${id}-panel`}
              tabIndex={active ? 0 : -1}
              onClick={() => select(id)}
              className={`flex flex-col items-center gap-3 rounded-2xl border px-4 py-5 text-sm font-medium transition-[background-color,border-color,color,transform] duration-200 active:scale-[0.98] ${
                active
                  ? "border-fd-foreground/25 bg-fd-card text-fd-foreground shadow-sm"
                  : "border-fd-border text-fd-muted-foreground hover:bg-fd-card/60 hover:text-fd-foreground"
              }`}
            >
              <OsIcon os={id} className="size-7" />
              {panelNames[id]}
              <span
                className={`-mt-2 text-[11px] font-normal text-fd-muted-foreground ${platform.os === id ? "" : "invisible"}`}
              >
                Your system
              </span>
            </button>
          );
        })}
      </div>
      {panel ? <PanelView key={panel.id} panel={panel} /> : null}
    </div>
  );
}
