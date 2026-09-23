// @ts-check
import starlight from "@astrojs/starlight";
import { defineConfig, passthroughImageService } from "astro/config";

// Served from GitHub Pages at https://teispace.github.io/teitunnel/ (D-053).
export default defineConfig({
  site: "https://teispace.github.io",
  base: "/teitunnel",
  // No image processing: the docs use SVG and pre-sized screenshots, so there's no
  // need for sharp (and its native install script).
  image: { service: passthroughImageService() },
  integrations: [
    starlight({
      title: "Teitunnel",
      description: "Cloudflare Tunnel, native on your Mac.",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/teispace/teitunnel" }],
      editLink: {
        baseUrl: "https://github.com/teispace/teitunnel/edit/main/apps/site/",
      },
      lastUpdated: true,
      customCss: ["./src/styles/custom.css"],
      sidebar: [
        {
          label: "Getting started",
          items: [
            "getting-started/install",
            "getting-started/quick-share",
            "getting-started/first-route",
          ],
        },
        {
          label: "Concepts",
          items: [
            "concepts/routes-and-tunnels",
            "concepts/changes",
            "concepts/run-modes",
            "concepts/dns-ownership",
          ],
        },
        {
          label: "Guides",
          items: [
            "guides/import",
            "guides/export",
            "guides/require-login",
            "guides/observability",
            "guides/menu-bar",
          ],
        },
        {
          label: "Reference",
          items: ["reference/cli", "reference/doctor", "reference/security", "reference/faq"],
        },
      ],
    }),
  ],
});
