/**
 * The questions and answers of a docs page written as `<Accordion title="…">…</Accordion>`,
 * as plain text for FAQ structured data, which search and AI answer engines read.
 */
export function faqFrom(mdx: string): { q: string; a: string }[] {
  return [...mdx.matchAll(/<Accordion title="([^"]+)">([\s\S]*?)<\/Accordion>/g)].map(
    ([, q, body]) => ({ q: q.trim(), a: plainText(body) }),
  );
}

/** Markdown and MDX as the words a reader sees. */
export function plainText(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[*_`]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
}
