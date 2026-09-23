import "./global.css";
import { RootProvider } from "fumadocs-ui/provider/next";
import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { motionBootScript } from "@/lib/motion-boot";
import { ogImage, pageMetadata } from "@/lib/seo";
import { asset, site } from "@/lib/site";

export const metadata: Metadata = {
  // Social cards default to the home page's; each page sets its own canonical URL.
  ...pageMetadata({ description: site.description, path: "/", image: ogImage(["home"]) }),
  alternates: {},
  title: { default: site.title, template: `%s · ${site.name}` },
  metadataBase: new URL(site.url),
  applicationName: site.name,
  keywords: site.keywords,
  authors: [{ name: site.org.name, url: site.org.url }],
  creator: site.org.name,
  publisher: site.org.name,
  category: "technology",
  robots: { index: true, follow: true, "max-image-preview": "large", "max-snippet": -1 },
  icons: { icon: asset("/icon.png"), apple: asset("/icon.png") },
  formatDetection: { telephone: false, email: false, address: false },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#ffffff" },
    { media: "(prefers-color-scheme: dark)", color: "#0a0a0a" },
  ],
  colorScheme: "light dark",
};

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        {/* biome-ignore lint/security/noDangerouslySetInnerHtml: a constant script, before first paint */}
        <script dangerouslySetInnerHTML={{ __html: motionBootScript }} />
      </head>
      <body className="flex min-h-screen flex-col antialiased">
        <RootProvider search={{ options: { type: "static", api: asset("/api/search") } }}>
          {children}
        </RootProvider>
      </body>
    </html>
  );
}
