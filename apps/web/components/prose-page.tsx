import type { ReactNode } from "react";

/** A plain text page, such as the privacy statement. */
export function ProsePage({
  title,
  updated,
  children,
}: {
  title: string;
  updated: string;
  children: ReactNode;
}) {
  return (
    <main className="mx-auto w-full max-w-3xl px-6 pt-16 pb-24 md:pt-24">
      <h1 className="text-3xl font-semibold tracking-tight md:text-5xl">{title}</h1>
      <p className="mt-3 text-sm text-fd-muted-foreground">Last updated {updated}</p>
      <div className="prose mt-10 max-w-none">{children}</div>
    </main>
  );
}
