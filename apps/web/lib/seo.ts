import type { Metadata } from "next";
import type { Release } from "./release.ts";
import { absolute, site } from "./site.ts";

/** Structured data (schema.org), rendered by `<JsonLd>`. */
export type Thing = Record<string, unknown>;

/** The social card of a page, generated at build time (`app/og`). */
export function ogImage(slug: string[]): string {
  return absolute(`/og/${[...slug, "image.png"].join("/")}`);
}

/**
 * A page's metadata with its canonical URL and social cards. `path` is the page's
 * address with its trailing slash, as the static export serves it.
 */
export function pageMetadata({
  title,
  description,
  path,
  image,
  type = "website",
  published,
  modified,
}: {
  title?: string;
  description: string;
  path: string;
  image: string;
  type?: "website" | "article";
  /** For articles: when it was first published and last changed (ISO 8601). */
  published?: string;
  modified?: string;
}): Metadata {
  const card = { url: image, width: 1200, height: 630, alt: title ?? site.name };
  const dates = type === "article" ? { publishedTime: published, modifiedTime: modified } : {};
  return {
    ...(title ? { title } : {}),
    description,
    alternates: { canonical: absolute(path) },
    openGraph: {
      type,
      ...dates,
      siteName: site.name,
      locale: "en_US",
      url: absolute(path),
      title: title ?? site.title,
      description,
      images: [card],
    },
    twitter: {
      card: "summary_large_image",
      title: title ?? site.title,
      description,
      images: [image],
    },
  };
}

export const organization: Thing = {
  "@type": "Organization",
  "@id": `${site.org.url}/#organization`,
  name: site.org.name,
  legalName: site.org.legalName,
  url: site.org.url,
  email: site.org.email,
  logo: absolute("/icon.png"),
  sameAs: ["https://github.com/teispace"],
};

/** The app, for search results that show software (name, price, systems). */
export function softwareApplication(release: Release | null): Thing {
  return {
    "@type": "SoftwareApplication",
    "@id": `${site.url}/#app`,
    name: site.name,
    description: site.description,
    url: site.url,
    image: absolute("/icon.png"),
    screenshot: absolute("/screens/routes-light.webp"),
    applicationCategory: "DeveloperApplication",
    applicationSubCategory: "Networking",
    operatingSystem: "macOS 14+, Windows 10, Windows 11, Linux",
    ...(release ? { softwareVersion: release.version, datePublished: release.date } : {}),
    downloadUrl: absolute("/download/"),
    installUrl: absolute("/download/"),
    releaseNotes: release?.notesUrl ?? site.releases,
    license: site.license,
    isAccessibleForFree: true,
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    publisher: { "@id": organization["@id"] },
    author: { "@id": organization["@id"] },
    codeRepository: site.github,
  };
}

export const website: Thing = {
  "@type": "WebSite",
  "@id": `${site.url}/#website`,
  name: site.name,
  url: site.url,
  description: site.description,
  inLanguage: "en",
  publisher: { "@id": organization["@id"] },
};

export function faqPage(items: { q: string; a: string }[]): Thing {
  return {
    "@type": "FAQPage",
    mainEntity: items.map(({ q, a }) => ({
      "@type": "Question",
      name: q,
      acceptedAnswer: { "@type": "Answer", text: a },
    })),
  };
}

export function breadcrumbs(items: { name: string; path: string }[]): Thing {
  return {
    "@type": "BreadcrumbList",
    itemListElement: items.map((item, index) => ({
      "@type": "ListItem",
      position: index + 1,
      name: item.name,
      item: absolute(item.path),
    })),
  };
}

export function techArticle({
  title,
  description,
  path,
  image,
  images = [],
  published,
  modified,
}: {
  title: string;
  description: string;
  path: string;
  image: string;
  /** The screenshots it shows. */
  images?: string[];
  published?: string;
  modified?: string;
}): Thing {
  return {
    "@type": "TechArticle",
    headline: title,
    description,
    url: absolute(path),
    mainEntityOfPage: absolute(path),
    image: [image, ...images],
    ...(published ? { datePublished: published } : {}),
    ...(modified ? { dateModified: modified } : {}),
    inLanguage: "en",
    isPartOf: { "@id": website["@id"] },
    about: { "@id": `${site.url}/#app` },
    author: { "@id": organization["@id"] },
    publisher: { "@id": organization["@id"] },
  };
}

/** Serializes structured data for a `<script>`; `<` is escaped so it can't close the tag. */
export function serializeJsonLd(things: Thing[]): string {
  const graph = { "@context": "https://schema.org", "@graph": things };
  return JSON.stringify(graph).replace(/</g, "\\u003c");
}
