/** Where things live, and how the site describes itself. */
export const site = {
  name: "Teitunnel",
  tagline: "Cloudflare Tunnel, done right.",
  /** The home page's title: what people search for, in plain words. */
  title: "Teitunnel: the Cloudflare Tunnel app for macOS, Windows and Linux",
  description:
    "A free, open-source app for Cloudflare Tunnel (cloudflared). Share localhost at a public URL in one click, publish apps on your own domain with DNS handled for you, add a login with Cloudflare Access, and keep tunnels running as a service. For macOS, Windows, Linux, servers and Docker.",
  keywords: [
    "Cloudflare Tunnel",
    "cloudflared",
    "Cloudflare Tunnel GUI",
    "cloudflared GUI",
    "Cloudflare Tunnel app",
    "expose localhost",
    "share localhost",
    "localhost tunnel",
    "ngrok alternative",
    "trycloudflare",
    "Cloudflare Zero Trust",
    "Cloudflare Access",
    "self-hosting",
    "home lab",
    "reverse tunnel",
  ],
  url: "https://teitunnel.teispace.com",
  repo: "teispace/teitunnel",
  github: "https://github.com/teispace/teitunnel",
  releases: "https://github.com/teispace/teitunnel/releases/latest",
  issues: "https://github.com/teispace/teitunnel/issues",
  license: "https://github.com/teispace/teitunnel/blob/main/LICENSE",
  org: {
    name: "Teispace",
    legalName: "Teispace Technology Private Limited",
    url: "https://teispace.com",
    email: "info@teispace.com",
  },
  basePath: process.env.NEXT_PUBLIC_BASE_PATH ?? "",
};

/** A path under the site's base (empty unless the site is hosted under a sub-path). */
export function asset(path: string): string {
  return `${site.basePath}${path}`;
}

/** The absolute URL of a page, as search engines should see it. */
export function absolute(path: string): string {
  return new URL(asset(path), site.url).toString();
}
