import { ImageResponse } from "next/og";
import { cards } from "@/lib/og-pages";
import { site } from "@/lib/site";

// Rendered once per page at build time (static export).
export const dynamic = "force-static";

interface Props {
  params: Promise<{ slug: string[] }>;
}

export async function GET(_request: Request, { params }: Props) {
  const { slug } = await params;
  const key = slug.slice(0, -1).join("/");
  const card = cards().find((c) => c.slug.join("/") === key);
  if (!card) return new Response("Not found", { status: 404 });
  return new ImageResponse(
    <div
      style={{
        width: "100%",
        height: "100%",
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
        padding: "72px 80px",
        backgroundColor: "#0a0a0b",
        backgroundImage:
          "radial-gradient(900px 480px at 85% -10%, rgba(56,132,255,0.38), transparent), radial-gradient(700px 420px at -10% 120%, rgba(56,132,255,0.16), transparent)",
        color: "#fafafa",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <svg width="52" height="52" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path
            d="M3 20V11a9 9 0 0 1 18 0v9"
            stroke="#fafafa"
            strokeWidth="2.2"
            strokeLinecap="round"
          />
          <path
            d="M8 20v-8a4 4 0 0 1 8 0v8"
            stroke="#fafafa"
            strokeWidth="2.2"
            strokeLinecap="round"
            opacity="0.55"
          />
        </svg>
        <span style={{ fontSize: 40, letterSpacing: -0.5 }}>{site.name}</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 22 }}>
        <span
          style={{
            fontSize: 24,
            textTransform: "uppercase",
            letterSpacing: 4,
            color: "#7fb0ff",
          }}
        >
          {card.eyebrow}
        </span>
        <span
          style={{
            fontSize: card.title.length > 40 ? 64 : 76,
            lineHeight: 1.08,
            letterSpacing: -2,
            maxWidth: 1000,
          }}
        >
          {card.title}
        </span>
        {card.description ? (
          <span style={{ fontSize: 30, lineHeight: 1.35, color: "#a1a1aa", maxWidth: 980 }}>
            {card.description.length > 150
              ? `${card.description.slice(0, 147).trimEnd()}…`
              : card.description}
          </span>
        ) : null}
      </div>
      <span style={{ fontSize: 24, color: "#71717a" }}>teitunnel.teispace.com</span>
    </div>,
    { width: 1200, height: 630 },
  );
}

export function generateStaticParams() {
  return cards().map((card) => ({ slug: [...card.slug, "image.png"] }));
}
