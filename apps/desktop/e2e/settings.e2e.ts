import { $, browser } from "@wdio/globals";

describe("Settings", () => {
  // On Windows, building a window from the main thread deadlocked in WebView2, so the
  // sidebar's Settings row did nothing there (0.2.0).
  it("opens from the sidebar, once, however often it's clicked", async () => {
    const main = await browser.getWindowHandle();
    const settings = $("button=Settings");
    await settings.click();

    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 2, {
      timeout: 15_000,
      timeoutMsg: "the Settings window didn't open",
    });
    await browser.switchToWindow(main);
    await settings.click();
    await browser.pause(1_000);
    const handles = await browser.getWindowHandles();
    if (handles.length !== 2) throw new Error(`expected 2 windows, got ${handles.length}`);

    const other = handles.find((handle) => handle !== main);
    if (!other) throw new Error("no Settings window handle");
    await browser.switchToWindow(other);
    await $("[role=tablist]").waitForExist({ timeout: 15_000 });

    await browser.closeWindow();
    await browser.switchToWindow(main);
  });
});
