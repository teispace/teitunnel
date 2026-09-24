"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { type ReactNode, useEffect, useRef } from "react";
import type { Download } from "@/lib/release";
import { buttonClass } from "./landing";

function Code({ children }: { children: string }) {
  return (
    <pre className="mt-2 overflow-x-auto rounded-lg border border-fd-border bg-fd-card px-4 py-3 font-mono text-sm">
      <code>{children}</code>
    </pre>
  );
}

/** Install steps for each kind of file. */
function steps(download: Download): ReactNode[] {
  const file = download.name;
  switch (download.kind) {
    case "dmg":
      return [
        <>
          Open <strong>{file}</strong> from your Downloads folder.
        </>,
        <>Drag Teitunnel into the Applications folder.</>,
        <>
          Open Teitunnel from Applications. macOS asks once whether to open an app from the
          internet: choose <strong>Open</strong>. It's signed and notarized, so that's all.
        </>,
      ];
    case "setup":
      return [
        <>
          Open <strong>{file}</strong> from your Downloads folder.
        </>,
        <>
          If Windows says it protected your PC, choose <strong>More info</strong>, then{" "}
          <strong>Run anyway</strong>. Windows signing through SignPath Foundation is on its way (
          <Link className="underline underline-offset-4" href="/code-signing/">
            code signing policy
          </Link>
          ).
        </>,
        <>
          Teitunnel installs for your user, without an administrator, and opens. Find it later in
          the Start menu.
        </>,
      ];
    case "deb":
      return [
        <>
          Install it with apt, which also installs what it needs:
          <Code>{`sudo apt install ./${file}`}</Code>
        </>,
        <>Open Teitunnel from your applications.</>,
      ];
    case "rpm":
      return [
        <>
          Install it with dnf (or zypper on openSUSE):<Code>{`sudo dnf install ./${file}`}</Code>
        </>,
        <>Open Teitunnel from your applications.</>,
      ];
    case "appimage":
      return [
        <>
          Make it executable and run it:<Code>{`chmod +x ${file}\n./${file}`}</Code>
        </>,
        <>
          It needs WebKitGTK 4.1 and FUSE 2 (<code>libfuse2</code>, or <code>libfuse2t64</code> on
          Ubuntu 24.04).
        </>,
      ];
    case "cli":
      return download.os === "linux"
        ? [
            <>
              Unpack it onto your PATH:
              <Code>{`sudo tar -xzf ${file} -C /usr/local/bin teitunnel-cli`}</Code>
            </>,
            <>
              Check it: <code>teitunnel-cli --version</code>, then see{" "}
              <Link className="underline underline-offset-4" href="/docs/guides/servers">
                Servers and containers
              </Link>
              .
            </>,
          ]
        : [
            <>
              Unzip <strong>{file}</strong> and move <code>teitunnel-cli</code> to a folder on your
              PATH.
            </>,
            <>
              Check it: <code>teitunnel-cli --version</code>, then see the{" "}
              <Link className="underline underline-offset-4" href="/docs/reference/cli">
                CLI reference
              </Link>
              .
            </>,
          ];
  }
}

/** Starts the download named in `?file=` (only files of the release) and explains the install. */
export function Thanks({ downloads }: { downloads: Download[] }) {
  const name = useSearchParams().get("file");
  const download = downloads.find((d) => d.name === name) ?? null;
  const started = useRef(false);

  useEffect(() => {
    if (!download || started.current) return;
    started.current = true;
    // GitHub serves it as an attachment, so the page stays.
    window.location.assign(download.url);
  }, [download]);

  if (!download) {
    return (
      <div className="flex flex-col gap-5">
        <h1 className="text-3xl font-semibold tracking-tight md:text-5xl">Choose a download</h1>
        <p className="text-lg text-fd-muted-foreground">
          That file isn't part of the latest release.
        </p>
        <Link href="/download/" className={`${buttonClass(true)} self-start`}>
          All downloads
        </Link>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-10">
      <header className="flex flex-col gap-4">
        <p className="font-mono text-xs uppercase tracking-[0.18em] text-fd-muted-foreground">
          Thanks for downloading
        </p>
        <h1 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">
          Your download is starting
        </h1>
        <p className="text-fd-muted-foreground">
          If it doesn't,{" "}
          <a className="underline underline-offset-4" href={download.url}>
            download {download.name}
          </a>
          .
        </p>
      </header>
      <section aria-labelledby="install">
        <h2 id="install" className="mb-4 text-xl font-semibold tracking-tight">
          Install it
        </h2>
        <ol className="flex flex-col gap-4">
          {steps(download).map((step, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: a fixed list of steps
            <li key={index} className="flex gap-4">
              <span className="flex size-7 shrink-0 items-center justify-center rounded-full border border-fd-border font-mono text-sm">
                {index + 1}
              </span>
              <div className="min-w-0 pt-0.5">{step}</div>
            </li>
          ))}
        </ol>
      </section>
      <section className="flex flex-wrap gap-3">
        <Link href="/docs/getting-started/quick-share" className={buttonClass(true)}>
          Get started
        </Link>
        <Link href="/download/" className={buttonClass(false)}>
          Other downloads
        </Link>
      </section>
    </div>
  );
}
