import { serializeJsonLd, type Thing } from "@/lib/seo";

/** Structured data for search engines. */
export function JsonLd({ things }: { things: Thing[] }) {
  return (
    <script
      type="application/ld+json"
      // biome-ignore lint/security/noDangerouslySetInnerHtml: escaped by serializeJsonLd
      dangerouslySetInnerHTML={{ __html: serializeJsonLd(things) }}
    />
  );
}
