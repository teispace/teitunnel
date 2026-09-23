import { HomeLayout } from "fumadocs-ui/layouts/home";
import type { ReactNode } from "react";
import { SiteFooter } from "@/components/landing";
import { baseOptions } from "@/lib/layout.shared";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <HomeLayout {...baseOptions()}>
      {children}
      <SiteFooter />
    </HomeLayout>
  );
}
