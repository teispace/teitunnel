import { $, browser, expect } from "@wdio/globals";
import { commandLines } from "./processes";

/** Command lines of running fake connectors (the app must never leave one behind). */
function fakeConnectors(): string[] {
  return commandLines().filter((line) => /fake-cloudflared tunnel .*--url/.test(line));
}

describe("Quick Share", () => {
  it("shares a port, shows the URL, and stops cleanly", async () => {
    await $("nav").$("a=Quick Share").click();
    const field = await $("[role=combobox][aria-label='Port or address']");
    await field.setValue("3000");
    await $("button=Share").click();

    const card = await $("article[aria-label='Quick Share of localhost:3000']");
    await card.waitForExist({ timeout: 15_000 });
    const url = await card.$("span*=trycloudflare.com");
    await url.waitForExist({ timeout: 15_000 });
    await expect(url).toHaveText(expect.stringMatching(/^https:\/\/fake-\d+\.trycloudflare\.com$/));
    // Live comes ~6 s after the URL (DNS propagation allowance).
    await card.$("span=Live").waitForExist({ timeout: 20_000 });
    expect(fakeConnectors()).toHaveLength(1);

    await card.$("button=Stop Sharing").click();
    await browser.waitUntil(() => fakeConnectors().length === 0, {
      timeout: 10_000,
      timeoutMsg: "fake-cloudflared is still running after Stop",
    });
    await card.waitForExist({ reverse: true, timeout: 10_000 });
  });
});
