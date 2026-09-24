import { site } from "./site";
import { source } from "./source";

/** A page with a social card: its eyebrow, title and description. */
export interface Card {
  slug: string[];
  eyebrow: string;
  title: string;
  description: string;
}

const sections: Record<string, string> = {
  "getting-started": "Getting started",
  concepts: "Concepts",
  tutorials: "Use cases",
  guides: "Guides",
  reference: "Reference",
};

/** The docs section a page is in, from its first slug ("Docs" for the index). */
export function sectionOf(slugs: string[]): string {
  return sections[slugs[0] ?? ""] ?? "Docs";
}

/** Every page's social card (`/og/<slug>/image.png`). */
export function cards(): Card[] {
  return [
    {
      slug: ["home"],
      eyebrow: "macOS · Windows · Linux · servers",
      title: site.tagline,
      description:
        "Share a local port in one click. Publish apps on your own domains. See every change before it happens.",
    },
    {
      slug: ["download"],
      eyebrow: "Download",
      title: "Download Teitunnel",
      description: "Free and open source, for macOS, Windows, Linux and servers.",
    },
    {
      slug: ["privacy"],
      eyebrow: "Privacy",
      title: "No telemetry. Nothing about you.",
      description: "What Teitunnel sends where.",
    },
    ...source.getPages().map((page) => ({
      slug: ["docs", ...page.slugs],
      eyebrow: sectionOf(page.slugs),
      title: page.data.title,
      description: page.data.description ?? "",
    })),
  ];
}
