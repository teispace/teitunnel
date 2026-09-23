/** Where things live. */
export const site = {
  name: "Teitunnel",
  tagline: "Cloudflare Tunnel, done right.",
  description:
    "Share a local port in one click, publish your apps on your own domains, and see every change before it happens. A native app, a CLI and a server mode for Cloudflare Tunnel. Free and open source.",
  url: "https://teitunnel.teispace.com",
  github: "https://github.com/teispace/teitunnel",
  releases: "https://github.com/teispace/teitunnel/releases/latest",
  basePath: process.env.NEXT_PUBLIC_BASE_PATH ?? "",
};

/** A path under the site's base (GitHub Pages serves it under /teitunnel). */
export function asset(path: string): string {
  return `${site.basePath}${path}`;
}
