import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
  EditOnGitHub,
} from "fumadocs-ui/layouts/docs/page";
import { createRelativeLink } from "fumadocs-ui/mdx";
import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { JsonLd } from "@/components/json-ld";
import { getMDXComponents } from "@/components/mdx";
import { sectionOf } from "@/lib/og-pages";
import { breadcrumbs, ogImage, pageMetadata, techArticle } from "@/lib/seo";
import { site } from "@/lib/site";
import { source } from "@/lib/source";

interface Props {
  params: Promise<{ slug?: string[] }>;
}

/** What a docs page is about, for its metadata and structured data. */
function describe(slugs: string[], title: string, description: string | undefined) {
  const path = `/docs/${slugs.map((s) => `${s}/`).join("")}`;
  return {
    path,
    description: description ?? site.description,
    image: ogImage(["docs", ...slugs]),
    // The index is "Introduction"; search results should say what it introduces.
    title: slugs.length === 0 ? "Teitunnel documentation" : title,
  };
}

export default async function Page({ params }: Props) {
  const { slug } = await params;
  const page = source.getPage(slug);
  if (!page) notFound();
  const MDX = page.data.body;
  const about = describe(page.slugs, page.data.title, page.data.description);
  const trail = [{ name: "Docs", path: "/docs/" }];
  if (page.slugs.length > 0) trail.push({ name: page.data.title, path: about.path });
  return (
    <DocsPage toc={page.data.toc} full={page.data.full}>
      <JsonLd
        things={[
          techArticle({ ...about, title: page.data.title }),
          breadcrumbs([{ name: site.name, path: "/" }, ...trail]),
        ]}
      />
      {page.slugs.length > 0 ? (
        <p className="mb-1 font-mono text-xs uppercase tracking-[0.18em] text-fd-muted-foreground">
          {sectionOf(page.slugs)}
        </p>
      ) : null}
      <DocsTitle>{page.data.title}</DocsTitle>
      <DocsDescription>{page.data.description}</DocsDescription>
      <DocsBody>
        <MDX components={getMDXComponents({ a: createRelativeLink(source, page) })} />
      </DocsBody>
      <EditOnGitHub href={`${site.github}/edit/main/apps/web/content/docs/${page.path}`} />
    </DocsPage>
  );
}

export function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params;
  const page = source.getPage(slug);
  if (!page) notFound();
  const about = describe(page.slugs, page.data.title, page.data.description);
  return pageMetadata({ ...about, type: "article" });
}
