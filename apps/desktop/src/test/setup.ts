// Vitest setup: DOM cleanup between tests, plus browser APIs jsdom lacks.
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(cleanup);
// `findBy…`/`waitFor` give up after 1 s by default, which a loaded CI runner can miss
// while the page is still settling; the assertion is the same, only more patient.
configure({ asyncUtilTimeout: 4000 });

class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub;
if (typeof Element !== "undefined") Element.prototype.scrollIntoView ??= () => {};
// uPlot reads the pixel ratio through matchMedia when it loads.
if (typeof window !== "undefined") {
  window.matchMedia ??= (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
      dispatchEvent: () => false,
    }) satisfies MediaQueryList;
}
// jsdom does no layout, so every element measures 0×0 and virtualized lists would
// render no rows. Give elements a plausible size instead.
if (typeof HTMLElement !== "undefined") {
  for (const [prop, value] of [
    ["offsetHeight", 480],
    ["offsetWidth", 640],
  ] as const) {
    if (
      Object.getOwnPropertyDescriptor(HTMLElement.prototype, prop)?.get?.call(document.body) === 0
    ) {
      Object.defineProperty(HTMLElement.prototype, prop, { configurable: true, get: () => value });
    }
  }
}
