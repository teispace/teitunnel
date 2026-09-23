import type { MetadataRoute } from "next";
import { absolute } from "@/lib/site";
import { source } from "@/lib/source";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const pages: [string, number][] = [
    ["/", 1],
    ["/download/", 0.9],
    ["/docs/", 0.9],
    ["/privacy/", 0.3],
    ["/code-signing/", 0.3],
  ];
  const docs = source
    .getPages()
    .filter((page) => page.slugs.length > 0)
    .map((page): [string, number] => [
      `${page.url}/`,
      page.slugs[0] === "getting-started" || page.slugs[0] === "tutorials" ? 0.8 : 0.7,
    ]);
  return [...pages, ...docs].map(([path, priority]) => ({
    url: absolute(path),
    changeFrequency: "weekly",
    priority,
  }));
}
