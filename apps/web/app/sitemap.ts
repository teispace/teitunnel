import type { MetadataRoute } from "next";
import { docDates, docScreenshots, gitDates } from "@/lib/page-facts";
import { ogImage } from "@/lib/seo";
import { absolute } from "@/lib/site";
import { source } from "@/lib/source";

export const dynamic = "force-static";

/**
 * Every page with when it last changed (from git) and the images it shows, so search
 * engines recrawl what changed and index the screenshots. `changeFrequency` and
 * `priority` are kept for the engines that still read them.
 */
export default function sitemap(): MetadataRoute.Sitemap {
  const home = (path: string, file: string, priority: number, images: string[]) => ({
    url: absolute(path),
    lastModified: gitDates(file).modified,
    changeFrequency: "weekly" as const,
    priority,
    images,
  });
  const pages = [
    home("/", "app/(home)/page.tsx", 1, [
      ogImage(["home"]),
      absolute("/screens/overview-light.webp"),
      absolute("/screens/overview-dark.webp"),
    ]),
    home("/download/", "app/(home)/download/page.tsx", 0.9, [ogImage(["download"])]),
    home("/docs/", "content/docs/index.mdx", 0.9, [ogImage(["docs"])]),
    home("/privacy/", "app/(home)/privacy/page.tsx", 0.3, []),
  ];
  const docs = source
    .getPages()
    .filter((page) => page.slugs.length > 0)
    .map((page) => ({
      url: absolute(`${page.url}/`),
      lastModified: docDates(page.path).modified,
      changeFrequency: "weekly" as const,
      priority: page.slugs[0] === "getting-started" || page.slugs[0] === "tutorials" ? 0.8 : 0.7,
      images: [ogImage(["docs", ...page.slugs]), ...docScreenshots(page.path)],
    }));
  return [...pages, ...docs];
}
