import "./global.css";
import { RootProvider } from "fumadocs-ui/provider/next";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import { asset, site } from "@/lib/site";

export const metadata: Metadata = {
  title: { default: `${site.name}: ${site.tagline}`, template: `%s · ${site.name}` },
  description: site.description,
  metadataBase: new URL("https://teispace.github.io"),
  openGraph: {
    title: site.name,
    description: site.description,
    images: [asset("/screens/routes-dark.png")],
  },
  icons: { icon: asset("/icon.png") },
};

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="flex min-h-screen flex-col antialiased">
        <RootProvider search={{ options: { type: "static", api: asset("/api/search") } }}>
          {children}
        </RootProvider>
      </body>
    </html>
  );
}
