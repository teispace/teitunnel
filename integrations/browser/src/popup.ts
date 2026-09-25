/**
 * The popup: share the local page you're on, see and stop your shares. Everything goes
 * through the Teitunnel app (which asks before sharing); text is set as text only.
 */

import { HostClient, HostError, type Share } from "./host.ts";
import { localOrigin } from "./local.ts";
import { explain, shareFor, shareLabel, shareTone } from "./view.ts";

const api = (typeof chrome !== "undefined" ? chrome : browser) as ExtensionApi;
const host = new HostClient(api.runtime);

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: { text?: string; className?: string } = {},
  ...children: Node[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (props.text !== undefined) node.textContent = props.text;
  if (props.className) node.className = props.className;
  node.append(...children);
  return node;
}

function byId<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`#${id} is missing`);
  return node as T;
}

function button(text: string, onClick: () => void, primary = false): HTMLButtonElement {
  const node = element("button", { text, className: primary ? "primary" : "" });
  node.type = "button";
  node.addEventListener("click", onClick);
  return node;
}

async function copy(text: string, from: HTMLButtonElement): Promise<void> {
  await navigator.clipboard.writeText(text);
  const before = from.textContent;
  from.textContent = "Copied";
  setTimeout(() => {
    from.textContent = before;
  }, 1200);
}

function say(error: unknown): void {
  const message = byId<HTMLParagraphElement>("message");
  const failure = error instanceof HostError ? error : new HostError("failed", String(error));
  const { text, action } = explain(failure);
  message.replaceChildren(element("span", { text }));
  if (action === "openApp") {
    message.append(
      " ",
      button("Open Teitunnel", () => void openApp()),
    );
  }
  message.hidden = false;
}

async function openApp(): Promise<void> {
  try {
    await host.request("open");
  } catch {
    // Not running: its link opens it.
    await api.tabs.create({ url: "teitunnel://open" });
  }
}

let origin: string | null = null;
let pageUrl: string | undefined;

function renderPage(shares: readonly Share[]): void {
  const page = byId<HTMLElement>("page");
  const existing = shareFor(origin, shares);
  if (!origin) {
    page.replaceChildren(
      element("p", {
        className: "hint",
        text: "Open a page from a server on this computer (like localhost:3000) to share it.",
      }),
    );
    return;
  }
  if (existing?.url) {
    const url = existing.url;
    const copyButton = button("Copy URL", () => void copy(url, copyButton), true);
    page.replaceChildren(
      element("p", { text: "This page is shared at" }),
      element("p", { className: "origin", text: url }),
      element("div", {}, copyButton),
    );
    return;
  }
  const share = button(
    `Share ${origin.replace(/^https?:\/\//, "")}`,
    () => void start(share),
    true,
  );
  page.replaceChildren(
    element("p", { text: "This page runs on this computer." }),
    element("p", { className: "hint", text: "Teitunnel asks you before it goes public." }),
    element("div", {}, share),
  );
}

function renderShares(shares: readonly Share[]): void {
  const list = byId<HTMLUListElement>("shares");
  if (shares.length === 0) {
    list.replaceChildren(element("li", { className: "hint", text: "Nothing is shared." }));
    return;
  }
  list.replaceChildren(
    ...shares.map((share) => {
      const actions = element("span");
      if (share.url) {
        const url = share.url;
        const copyButton = button("Copy", () => void copy(url, copyButton));
        actions.append(copyButton, " ");
      }
      const stop = button("Stop", () => {
        stop.disabled = true;
        host
          .request("shares.stop", { id: share.id })
          .then(refresh)
          .catch((error: unknown) => {
            stop.disabled = false;
            say(error);
          });
      });
      actions.append(stop);
      return element(
        "li",
        {},
        element("span", { className: `dot ${shareTone(share)}` }),
        element(
          "span",
          { className: "label", text: shareLabel(share) },
          element("small", { text: share.origin.replace(/^https?:\/\//, "") }),
        ),
        actions,
      );
    }),
  );
}

async function refresh(): Promise<void> {
  try {
    const shares = await host.request<Share[]>("shares.list");
    byId<HTMLParagraphElement>("message").hidden = true;
    renderPage(shares);
    renderShares(shares);
    // A share still getting its URL: look again shortly.
    if (shares.some((s) => !s.url && s.status !== "failed")) setTimeout(() => void refresh(), 2000);
  } catch (error) {
    renderPage([]);
    byId<HTMLUListElement>("shares").replaceChildren();
    say(error);
  }
}

async function start(from: HTMLButtonElement): Promise<void> {
  from.disabled = true;
  from.textContent = "Waiting for your approval in Teitunnel…";
  try {
    const share = await host.request<Share>("shares.start", { url: pageUrl });
    if (share.url) await navigator.clipboard.writeText(share.url).catch(() => undefined);
  } catch (error) {
    say(error);
  }
  await refresh();
}

async function main(): Promise<void> {
  const [tab] = await api.tabs.query({ active: true, currentWindow: true });
  pageUrl = tab?.url;
  origin = localOrigin(pageUrl);
  byId<HTMLButtonElement>("open").addEventListener("click", () => void openApp());
  await refresh();
}

void main();
