/**
 * IndexNow: after a deploy, tell the search engines that take part (Bing, Yandex, Seznam,
 * Naver; Bing's index also serves ChatGPT search and Copilot) which pages changed, so
 * they're recrawled within minutes instead of days. <https://www.indexnow.org/documentation>
 */

/** The pages of a sitemap that changed since `since` (from their `lastmod`). */
export function changedSince(sitemap: string, since: Date): string[] {
  const urls: string[] = [];
  for (const [, entry] of sitemap.matchAll(/<url>([\s\S]*?)<\/url>/g)) {
    const loc = entry.match(/<loc>([^<]+)<\/loc>/)?.[1];
    const lastmod = entry.match(/<lastmod>([^<]+)<\/lastmod>/)?.[1];
    if (loc && lastmod && Date.parse(lastmod) >= since.getTime()) urls.push(loc);
  }
  return urls;
}

/** The request body for `urls` on `host`, whose key file is `/<key>.txt`. */
export function submission(host: string, key: string, urls: string[]) {
  return {
    host,
    key,
    keyLocation: `https://${host}/${key}.txt`,
    // IndexNow takes up to 10,000 URLs per request.
    urlList: urls.slice(0, 10_000),
  };
}
