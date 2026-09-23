import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";
import { Logo } from "@/components/logo";
import { site } from "./site";

/** Navigation shared by the landing page and the docs. */
export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="flex items-center gap-2 font-semibold">
          <Logo className="size-5" />
          {site.name}
        </span>
      ),
    },
    githubUrl: site.github,
    links: [
      { text: "Docs", url: "/docs", active: "nested-url" },
      { text: "Download", url: "/download/" },
    ],
  };
}
