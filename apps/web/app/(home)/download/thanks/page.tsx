import type { Metadata } from "next";
import { Suspense } from "react";
import { Thanks } from "@/components/thanks";
import { latestRelease } from "@/lib/release";

export const metadata: Metadata = {
  title: "Your download is starting",
  description: "How to install Teitunnel on your system.",
  robots: { index: false },
};

export default async function ThanksPage() {
  const release = await latestRelease();
  return (
    <main className="mx-auto w-full max-w-3xl px-6 pt-16 pb-24 md:pt-24">
      {/* The file comes from the address (`?file=`), known only in the browser. */}
      <Suspense>
        <Thanks downloads={release?.downloads ?? []} />
      </Suspense>
    </main>
  );
}
