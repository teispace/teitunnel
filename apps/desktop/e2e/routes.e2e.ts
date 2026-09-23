import { $, browser, expect } from "@wdio/globals";
import { commandLines } from "./processes";

/** Running fake connectors for named tunnels (`tunnel … run`). */
function tunnelConnectors(): string[] {
  return commandLines().filter((line) => /fake-cloudflared tunnel .* run$/.test(line));
}

describe("Routes", () => {
  it("connects an account, adds a verified route, then removes everything", async () => {
    await $("nav").$("a=Routes").click();

    // Connect with an API token (the fake Cloudflare accepts any but "bad").
    await $("button=Connect Cloudflare").click();
    const sheet = await $("[role=dialog]");
    await sheet.$("input[type=password]").setValue("e2e-token");
    await sheet.$("button=Connect").click();

    // No routes yet: add one.
    await $("button=Add Route").waitForExist({ timeout: 15_000 });
    await $("button=Add Route").click();
    const dialog = await $("[role=dialog][aria-labelledby]");
    await dialog.$("[role=combobox][aria-label='Port or address']").setValue("3000");
    await dialog.$("input[aria-label=Subdomain]").setValue("app");
    await dialog.$("button=Review").click();

    await dialog.$("span*=Create tunnel").waitForExist({ timeout: 15_000 });
    await dialog.$("button=Add Route").click();
    await dialog.$("p=It works").waitForExist({ timeout: 30_000 });
    await dialog.$("button=Done").click();

    const row = await $("//*[@role='option'][contains(., 'app.xyz.com')]");
    await row.waitForExist({ timeout: 10_000 });
    await browser.waitUntil(() => tunnelConnectors().length === 1, {
      timeout: 15_000,
      timeoutMsg: "the tunnel connector didn't start",
    });

    // Remove the route, then the tunnel: nothing may be left running.
    await $("button[aria-label='Remove route']").click();
    const remove = await $("[role=dialog][aria-labelledby]");
    await remove.$("span*=Delete DNS record").waitForExist({ timeout: 15_000 });
    await remove.$("button=Remove").click();
    await row.waitForExist({ reverse: true, timeout: 15_000 });

    await $("nav").$("a=Tunnels").click();
    // The tunnel list loads after navigating; wait for it rather than racing it.
    const deleteTunnel = await $("button=Delete…");
    await deleteTunnel.waitForDisplayed({ timeout: 15_000 });
    await deleteTunnel.scrollIntoView();
    await deleteTunnel.click();
    const del = await $("[role=dialog][aria-labelledby]");
    await del.$("span*=Delete tunnel").waitForExist({ timeout: 15_000 });
    await del.$("button=Delete Tunnel").click();
    await $("h2=No tunnels").waitForExist({ timeout: 15_000 });
    await browser.waitUntil(() => tunnelConnectors().length === 0, {
      timeout: 10_000,
      timeoutMsg: "the tunnel connector is still running",
    });
    await expect($("h2=No tunnels")).toBeDisplayed();
  });
});
