import { readFileSync } from "node:fs";
import { join } from "node:path";
import { sectionOf } from "./og-pages";
import { absolute, site } from "./site";
import { source } from "./source";

/** A docs page's MDX without its front matter. */
function body(path: string): string {
  const raw = readFileSync(join(process.cwd(), "content/docs", path), "utf8");
  return raw.replace(/^---\n[\s\S]*?\n---\n/, "").trim();
}

/** `llms.txt`: what Teitunnel is and where each docs page is (https://llmstxt.org). */
export function llmsIndex(): string {
  const groups = new Map<string, string[]>();
  for (const page of source.getPages()) {
    const section = sectionOf(page.slugs);
    const line = `- [${page.data.title}](${absolute(`${page.url}/`)}): ${page.data.description ?? ""}`;
    groups.set(section, [...(groups.get(section) ?? []), line]);
  }
  const sections = [...groups].map(([name, lines]) => `## ${name}\n\n${lines.join("\n")}`);
  return [
    `# ${site.name}`,
    `> ${site.description}`,
    `Teitunnel is made by ${site.org.name} and is not affiliated with Cloudflare. Downloads: ${absolute("/download/")}. Source: ${site.github}. The full docs as one file: ${absolute("/llms-full.txt")}.`,
    `For AI agents: Teitunnel has an MCP server (\`teitunnel mcp\`, connect a client with \`teitunnel mcp install <client>\`; see ${absolute("/docs/guides/ai-agents/")}) and an Agent Skill at ${absolute("/skills/teitunnel/SKILL.md")}.`,
    ...sections,
  ].join("\n\n");
}

/** `llms-full.txt`: every docs page in one file. */
export function llmsFull(): string {
  const pages = source.getPages().map((page) => {
    const url = absolute(`${page.url}/`);
    return `# ${page.data.title}\n\nURL: ${url}\n\n${page.data.description ?? ""}\n\n${body(page.path)}`;
  });
  return [`# ${site.name} documentation\n\n> ${site.description}`, ...pages].join("\n\n---\n\n");
}
