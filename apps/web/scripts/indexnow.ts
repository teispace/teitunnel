// Submits the pages of the live sitemap that changed in the last day to IndexNow. Run by
// the docs workflow after a deploy: `node apps/web/scripts/indexnow.ts`. The key is the
// name of the key file the site serves (`public/<key>.txt`), public by design.
import { readdirSync } from "node:fs";
import { changedSince, submission } from "../lib/indexnow.ts";

const HOST = "teitunnel.teispace.com";
const key = readdirSync(new URL("../public/", import.meta.url))
  .map((name) => name.match(/^([0-9a-f]{32})\.txt$/)?.[1])
  .find(Boolean);
if (!key) throw new Error("no IndexNow key file in apps/web/public");

const sitemap = await (await fetch(`https://${HOST}/sitemap.xml`)).text();
const urls = changedSince(sitemap, new Date(Date.now() - 24 * 60 * 60 * 1000));
if (urls.length === 0) {
  process.stdout.write("IndexNow: no page changed in the last day\n");
} else {
  const response = await fetch("https://api.indexnow.org/indexnow", {
    method: "POST",
    headers: { "content-type": "application/json; charset=utf-8" },
    body: JSON.stringify(submission(HOST, key, urls)),
  });
  // 200 and 202 are success; anything else is reported, never fatal to the deploy.
  process.stdout.write(`IndexNow: ${urls.length} pages, HTTP ${response.status}\n`);
}
