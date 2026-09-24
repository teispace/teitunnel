import type { DevServer } from "@/lib/ipc/bindings";

/** Framework names, as their authors write them (not translated). */
const names: Record<DevServer, string> = {
  vite: "Vite",
  svelteKit: "SvelteKit",
  astro: "Astro",
  nuxt: "Nuxt",
  angular: "Angular",
  webpack: "webpack-dev-server",
  next: "Next.js",
  rails: "Rails",
  django: "Django",
};

export function devServerName(server: DevServer): string {
  return names[server];
}
