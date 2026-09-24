// Teitunnel's offline page (docs/research/cloudflare-workers-features.md).
//
// A Worker route on hostname/* sends every request on to the tunnel unchanged
// (fetch(request): the origin in DNS). When the tunnel has no connector, Cloudflare
// answers 530 (error 1033): this computer is off. Then visitors get the page from the
// PAGE binding ({title, message, whenAppDown}) instead of Cloudflare's error; API and
// asset requests get a short JSON or text answer. With whenAppDown, a 502/504 (the
// tunnel is up but the local app doesn't answer) shows the page too.
//
// It never buffers bodies and never changes a working response. Keep it small; tested
// in apps/desktop/src/test/front-workers.test.ts.

function escapeHtml(text) {
  return String(text).replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
  );
}

export function settings(env) {
  try {
    const page = JSON.parse(env.PAGE || "{}");
    return {
      title: String(page.title || "Back soon"),
      message: String(page.message || ""),
      whenAppDown: page.whenAppDown === true,
    };
  } catch {
    return { title: "Back soon", message: "", whenAppDown: false };
  }
}

/** Whether a response from the tunnel means the site is unavailable. */
export function isOffline(status, page) {
  return status === 530 || (page.whenAppDown && (status === 502 || status === 504));
}

export function wantsPage(request) {
  if (request.method !== "GET" && request.method !== "HEAD") return false;
  const dest = request.headers.get("Sec-Fetch-Dest");
  if (dest) return dest === "document";
  return (request.headers.get("Accept") || "").includes("text/html");
}

export function offlinePage(page, request) {
  const headers = {
    "Cache-Control": "no-store",
    "Retry-After": "60",
    "X-Robots-Tag": "noindex",
  };
  if (!wantsPage(request)) {
    return new Response(JSON.stringify({ error: "offline", message: page.title }), {
      status: 503,
      headers: { ...headers, "Content-Type": "application/json; charset=utf-8" },
    });
  }
  const paragraphs = page.message
    .split(/\n+/)
    .filter(Boolean)
    .map((line) => `<p>${escapeHtml(line)}</p>`)
    .join("");
  const body = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="robots" content="noindex"><title>${escapeHtml(page.title)}</title><style>body{font:16px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;margin:0;min-height:100vh;display:grid;place-items:center;color:#1d1d1f;background:#f5f5f7}main{max-width:32rem;padding:2rem}h1{font-size:1.5rem;margin:0 0 .5rem}p{margin:.5rem 0;color:#515154}@media(prefers-color-scheme:dark){body{color:#f5f5f7;background:#1d1d1f}p{color:#a1a1a6}}</style></head><body><main><h1>${escapeHtml(page.title)}</h1>${paragraphs}</main></body></html>`;
  return new Response(request.method === "HEAD" ? null : body, {
    status: 503,
    headers: {
      ...headers,
      "Content-Type": "text/html; charset=utf-8",
      "Content-Security-Policy": "default-src 'none'; style-src 'unsafe-inline'",
    },
  });
}

export default {
  async fetch(request, env) {
    const page = settings(env);
    let response;
    try {
      response = await fetch(request);
    } catch {
      return offlinePage(page, request);
    }
    return isOffline(response.status, page) ? offlinePage(page, request) : response;
  },
};
