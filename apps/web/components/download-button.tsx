"use client";

import { Download as DownloadIcon } from "lucide-react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { detectPlatform } from "@/lib/detect";
import { type Choice, choose, thanksHref } from "@/lib/pick";
import type { Download } from "@/lib/release";
import { buttonClass } from "./landing";

/**
 * The main download button, for the visitor's system. It renders as a link to the
 * download page (static HTML, and without JavaScript), then becomes the right file.
 */
export function DownloadButton({
  downloads,
  showAlternative = true,
}: {
  downloads: Download[];
  showAlternative?: boolean;
}) {
  const [choice, setChoice] = useState<Choice | null>(null);
  useEffect(() => {
    let live = true;
    void detectPlatform().then((platform) => {
      if (live) setChoice(choose(platform, downloads));
    });
    return () => {
      live = false;
    };
  }, [downloads]);

  const href = choice?.download ? thanksHref(choice.download) : "/download/";
  return (
    <span className="inline-flex flex-wrap items-center gap-x-4 gap-y-2">
      <Link href={href} className={buttonClass(true)}>
        <DownloadIcon className="size-4" aria-hidden />
        {choice?.label ?? "Download"}
      </Link>
      {showAlternative && choice?.alternative ? (
        <Link
          href={choice.alternative.href}
          className="text-sm text-fd-muted-foreground underline-offset-4 hover:text-fd-foreground hover:underline"
        >
          {choice.alternative.label}
        </Link>
      ) : null}
    </span>
  );
}
