import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { breadcrumbs, faqPage, ogImage, pageMetadata, serializeJsonLd } from "./seo.ts";

describe("seo", () => {
  it("escapes markup so structured data can't end its script tag", () => {
    const json = serializeJsonLd([{ "@type": "Thing", name: "</script><script>alert(1)" }]);
    assert.ok(!json.includes("</script>"));
    assert.deepEqual(JSON.parse(json)["@graph"][0].name, "</script><script>alert(1)");
    assert.equal(JSON.parse(json)["@context"], "https://schema.org");
  });

  it("gives every page an absolute canonical URL and a social card", () => {
    const image = ogImage(["docs", "getting-started", "install"]);
    assert.equal(image, "https://teitunnel.teispace.com/og/docs/getting-started/install/image.png");
    const meta = pageMetadata({
      title: "Install",
      description: "d",
      path: "/docs/install/",
      image,
    });
    assert.equal(meta.alternates?.canonical, "https://teitunnel.teispace.com/docs/install/");
    assert.equal(
      (meta.openGraph as { url: string }).url,
      "https://teitunnel.teispace.com/docs/install/",
    );
    assert.equal(meta.title, "Install");
  });

  it("numbers breadcrumbs from one with absolute URLs", () => {
    const list = breadcrumbs([
      { name: "Docs", path: "/docs/" },
      { name: "Install", path: "/docs/install/" },
    ]);
    const items = list.itemListElement as { position: number; item: string }[];
    assert.deepEqual(
      items.map((i) => [i.position, i.item]),
      [
        [1, "https://teitunnel.teispace.com/docs/"],
        [2, "https://teitunnel.teispace.com/docs/install/"],
      ],
    );
  });

  it("lists questions with their answers", () => {
    const faq = faqPage([{ q: "Free?", a: "Yes." }]);
    assert.deepEqual(faq.mainEntity, [
      { "@type": "Question", name: "Free?", acceptedAnswer: { "@type": "Answer", text: "Yes." } },
    ]);
  });
});
