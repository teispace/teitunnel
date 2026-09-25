// Teitunnel comments overlay (M12-06, docs/guides: comments).
//
// Served at /__teitunnel/comments/overlay.js by Lens (live shares) and by a Snapshot's
// Worker, and added to HTML pages as <script src=… defer>. Reviewers pin comments to a
// spot on the page, reply and resolve. Everything lives in a closed Shadow DOM, is
// built with DOM APIs (text is only ever set with textContent), styles come from a
// constructable stylesheet (allowed under a strict style-src) and nothing is loaded
// from elsewhere. The API is same-origin JSON under ./api/.
//
// Keep it small and dependency-free. Tested in apps/desktop/src/test/comments-overlay.test.ts
// (set globalThis.__TEITUNNEL_COMMENTS_TEST__ = {} before evaluating to get the helpers
// instead of mounting).
(() => {
  const TEST = globalThis.__TEITUNNEL_COMMENTS_TEST__;
  if (!TEST && (window.top !== window || window.__teitunnelComments)) return;

  const NAME_KEY = "teitunnel.comments.name";
  const MAX_BODY = 4000;
  const MAX_NAME = 80;
  const POLL_MS = 30000;
  const HASH = "#__teitunnel-comment=";

  // ---------- pure helpers (exported for tests) ----------

  function escapeIdent(value) {
    if (globalThis.CSS && typeof CSS.escape === "function") return CSS.escape(value);
    return String(value).replace(/[^a-zA-Z0-9_-]/g, (c) => `\\${c}`);
  }

  /** A short CSS selector for `el`: an id when it's unique, else a path of nth-of-type steps. */
  function cssPath(el, root) {
    const doc = el.ownerDocument;
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== root && parts.length < 8) {
      const id = node.getAttribute("id");
      if (
        id &&
        /^[A-Za-z][\w-]{0,63}$/.test(id) &&
        doc.querySelectorAll(`#${escapeIdent(id)}`).length === 1
      ) {
        parts.unshift(`#${escapeIdent(id)}`);
        break;
      }
      const tag = node.localName;
      if (tag === "body" || tag === "html") {
        parts.unshift(tag);
        break;
      }
      let index = 1;
      for (let sib = node.previousElementSibling; sib; sib = sib.previousElementSibling) {
        if (sib.localName === tag) index++;
      }
      parts.unshift(`${tag}:nth-of-type(${index})`);
      node = node.parentElement;
    }
    const selector = parts.join(" > ");
    return selector.length <= 512
      ? selector
      : selector.slice(selector.length - 512).replace(/^[^>]*>\s*/, "");
  }

  function clamp01(n) {
    return Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0;
  }

  /** Where a click landed: the element's selector and the point inside it (fractions). */
  function anchorFor(el, clientX, clientY, win) {
    const rect = el.getBoundingClientRect();
    const w = win || window;
    return {
      selector: cssPath(el),
      x: rect.width > 0 ? clamp01((clientX - rect.left) / rect.width) : 0,
      y: rect.height > 0 ? clamp01((clientY - rect.top) / rect.height) : 0,
      left: Math.max(0, Math.round(clientX + w.scrollX)),
      top: Math.max(0, Math.round(clientY + w.scrollY)),
      vw: Math.round(w.innerWidth),
      vh: Math.round(w.innerHeight),
    };
  }

  /** The anchor's point in viewport coordinates, or null when the page has no such spot. */
  function resolveAnchor(anchor, doc, win) {
    if (!anchor) return null;
    const d = doc || document;
    const w = win || window;
    let el = null;
    try {
      el = anchor.selector ? d.querySelector(anchor.selector) : null;
    } catch {
      el = null;
    }
    if (el) {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 || rect.height > 0) {
        return {
          x: rect.left + anchor.x * rect.width,
          y: rect.top + anchor.y * rect.height,
          exact: true,
        };
      }
    }
    if (Number.isFinite(anchor.left) && Number.isFinite(anchor.top)) {
      return { x: anchor.left - w.scrollX, y: anchor.top - w.scrollY, exact: false };
    }
    return null;
  }

  /** "just now", "5 min ago", "3 hr ago", or a date. */
  function relativeTime(ms, now) {
    const seconds = Math.max(0, Math.round(((now ?? Date.now()) - ms) / 1000));
    if (seconds < 45) return "just now";
    if (seconds < 3600) return `${Math.round(seconds / 60)} min ago`;
    if (seconds < 86400) return `${Math.round(seconds / 3600)} hr ago`;
    return new Date(ms).toLocaleDateString();
  }

  /** A name as the server accepts it: trimmed, no control characters, at most 80 chars. */
  function cleanName(name) {
    const plain = Array.from(String(name ?? ""), (c) => {
      const n = c.codePointAt(0);
      return n < 32 || n === 127 ? " " : c;
    }).join("");
    return Array.from(plain.trim()).slice(0, MAX_NAME).join("");
  }

  function hashTarget(hash) {
    return typeof hash === "string" && hash.startsWith(HASH)
      ? decodeURIComponent(hash.slice(HASH.length))
      : null;
  }

  if (TEST) {
    Object.assign(TEST, {
      cssPath,
      anchorFor,
      resolveAnchor,
      relativeTime,
      cleanName,
      hashTarget,
      mount,
    });
    return;
  }

  mount({});

  // ---------- the overlay ----------

  function mount(options) {
    const doc = options.document || document;
    const win = options.window || window;
    const fetcher = options.fetch || win.fetch.bind(win);
    const script = options.base ? null : doc.currentScript;
    const base =
      options.base ||
      new URL(".", script ? script.src : `${win.location.origin}/__teitunnel/comments/`).pathname;
    const api = `${base}api/`;
    const reduceMotion = win.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

    const state = {
      threads: [],
      open: null,
      placing: false,
      showResolved: false,
      me: { verified: false },
      error: "",
    };

    const host = doc.createElement("div");
    host.setAttribute("data-teitunnel-comments", "");
    host.style.setProperty("all", "initial");
    host.style.setProperty("position", "fixed");
    host.style.setProperty("inset", "0");
    host.style.setProperty("pointer-events", "none");
    host.style.setProperty("z-index", "2147483646");
    const shadow = host.attachShadow({ mode: options.openShadow ? "open" : "closed" });
    applyStyles(shadow, doc);

    const el = (tag, attrs, ...children) => {
      const node = doc.createElement(tag);
      for (const [key, value] of Object.entries(attrs || {})) {
        if (value === false || value == null) continue;
        if (key === "class") node.className = value;
        else if (key === "text") node.textContent = value;
        else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
        else node.setAttribute(key, value === true ? "" : String(value));
      }
      for (const child of children) if (child) node.append(child);
      return node;
    };
    const icon = (path) => {
      const svg = doc.createElementNS("http://www.w3.org/2000/svg", "svg");
      svg.setAttribute("viewBox", "0 0 16 16");
      svg.setAttribute("aria-hidden", "true");
      const p = doc.createElementNS("http://www.w3.org/2000/svg", "path");
      p.setAttribute("d", path);
      svg.append(p);
      return svg;
    };

    const layer = el("div", { class: "layer" });
    const highlight = el("div", { class: "highlight", hidden: true });
    const live = el("div", { class: "sr", role: "status", "aria-live": "polite" });
    const count = el("span", { class: "count", text: "" });
    const toggle = el(
      "button",
      {
        class: "fab",
        type: "button",
        "aria-label": "Comments",
        "aria-expanded": "false",
        onclick: () => togglePanel(),
      },
      icon(
        "M2 3.5A1.5 1.5 0 0 1 3.5 2h9A1.5 1.5 0 0 1 14 3.5v6a1.5 1.5 0 0 1-1.5 1.5H7l-3 3v-3h-.5A1.5 1.5 0 0 1 2 9.5z",
      ),
      count,
    );
    const panel = el("section", {
      class: "panel",
      role: "dialog",
      "aria-label": "Comments on this page",
      hidden: true,
    });
    const hint = el("div", { class: "hint", role: "status", hidden: true });
    shadow.append(layer, highlight, hint, panel, toggle, live);
    doc.documentElement.append(host);

    const announce = (text) => {
      live.textContent = "";
      win.setTimeout(() => (live.textContent = text), 30);
    };

    async function call(method, path, body) {
      const response = await fetcher(api + path, {
        method,
        credentials: "same-origin",
        headers: body
          ? { "Content-Type": "application/json", Accept: "application/json" }
          : { Accept: "application/json" },
        body: body ? JSON.stringify(body) : undefined,
      });
      const data = await response.json().catch(() => ({}));
      if (!response.ok)
        throw new Error(data.error || `Couldn't reach the comments (${response.status})`);
      return data;
    }

    async function load() {
      try {
        const data = await call("GET", `threads?path=${encodeURIComponent(win.location.pathname)}`);
        state.threads = Array.isArray(data.threads) ? data.threads : [];
        state.me = data.me || { verified: false };
        state.error = "";
      } catch (error) {
        state.error = error.message;
      }
      render();
    }

    function upsert(thread) {
      const index = state.threads.findIndex((t) => t.id === thread.id);
      if (index >= 0) state.threads[index] = thread;
      else state.threads.push(thread);
    }

    function storedName() {
      try {
        return win.localStorage.getItem(NAME_KEY) || "";
      } catch {
        return "";
      }
    }
    function rememberName(name) {
      try {
        win.localStorage.setItem(NAME_KEY, name);
      } catch {
        // Private mode: ask again next time.
      }
    }

    // ----- rendering -----

    function visibleThreads() {
      return state.threads.filter((t) => state.showResolved || !t.resolved);
    }

    function render() {
      const open = state.threads.filter((t) => !t.resolved).length;
      count.textContent = open ? String(open) : "";
      toggle.setAttribute("aria-label", open ? `Comments, ${open} open` : "Comments");
      renderPins();
      if (!panel.hidden) renderPanel();
    }

    function renderPins() {
      layer.replaceChildren();
      visibleThreads().forEach((thread, index) => {
        const point = resolveAnchor(thread.anchor, doc, win);
        if (!point) return;
        const first = thread.comments[0];
        const pin = el("button", {
          class: `pin${thread.resolved ? " resolved" : ""}${state.open === thread.id ? " active" : ""}`,
          type: "button",
          "aria-label": `Comment ${index + 1} by ${first ? first.author : ""}${thread.resolved ? ", resolved" : ""}`,
          text: String(index + 1),
          onclick: (event) => {
            event.stopPropagation();
            openThread(thread.id, pin);
          },
        });
        pin.style.setProperty("left", `${Math.round(point.x)}px`);
        pin.style.setProperty("top", `${Math.round(point.y)}px`);
        layer.append(pin);
      });
    }

    function commentView(comment) {
      return el(
        "li",
        { class: "comment" },
        el(
          "div",
          { class: "meta" },
          el("strong", { text: comment.author }),
          comment.byOwner ? el("span", { class: "badge", text: "Owner" }) : null,
          comment.verified ? el("span", { class: "badge", text: "Signed in" }) : null,
          el("time", {
            datetime: new Date(comment.createdAt).toISOString(),
            text: relativeTime(comment.createdAt),
          }),
        ),
        el("p", { class: "body", text: comment.body }),
      );
    }

    function renderPanel() {
      const list = visibleThreads();
      const items = list.map((thread, index) =>
        el(
          "li",
          {},
          el(
            "button",
            { class: "row", type: "button", onclick: () => openThread(thread.id) },
            el("span", {
              class: `num${thread.resolved ? " resolved" : ""}`,
              text: String(index + 1),
            }),
            el("span", {
              class: "excerpt",
              text: thread.comments[0] ? thread.comments[0].body : "",
            }),
            el("span", {
              class: "sub",
              text: `${thread.comments.length > 1 ? `${thread.comments.length - 1} replies · ` : ""}${thread.resolved ? "Resolved" : relativeTime(thread.createdAt)}`,
            }),
          ),
        ),
      );
      panel.replaceChildren(
        el(
          "header",
          {},
          el("h2", { text: "Comments" }),
          el(
            "button",
            {
              class: "icon",
              type: "button",
              "aria-label": "Close",
              onclick: () => togglePanel(false),
            },
            icon("M4 4l8 8M12 4l-8 8"),
          ),
        ),
        el("button", {
          class: "primary",
          type: "button",
          onclick: startPlacing,
          text: "Add a comment",
        }),
        el(
          "label",
          { class: "check" },
          el("input", {
            type: "checkbox",
            checked: state.showResolved,
            onchange: (event) => {
              state.showResolved = event.target.checked;
              render();
            },
          }),
          el("span", { text: "Show resolved" }),
        ),
        state.error ? el("p", { class: "error", role: "alert", text: state.error }) : null,
        items.length
          ? el("ol", { class: "threads" }, ...items)
          : el("p", { class: "empty", text: "No comments on this page yet." }),
      );
    }

    function togglePanel(force) {
      const show = force ?? panel.hidden;
      panel.hidden = !show;
      toggle.setAttribute("aria-expanded", String(show));
      if (show) {
        renderPanel();
        panel.querySelector("button.primary")?.focus();
      } else toggle.focus();
    }

    // ----- placing a comment -----

    function stopPlacing() {
      state.placing = false;
      hint.hidden = true;
      highlight.hidden = true;
      doc.removeEventListener("mousemove", onHover, true);
      doc.removeEventListener("click", onPlace, true);
      doc.removeEventListener("keydown", onPlaceKey, true);
    }

    function startPlacing() {
      closePopover();
      state.placing = true;
      hint.replaceChildren(
        el("span", { text: "Click the spot you want to comment on." }),
        el("button", {
          type: "button",
          class: "link",
          onclick: () => compose(null, win.innerWidth / 2, win.innerHeight / 3),
          text: "Comment on the whole page",
        }),
        el("button", { type: "button", class: "link", onclick: stopPlacing, text: "Cancel" }),
      );
      hint.hidden = false;
      hint.querySelector("button")?.focus();
      doc.addEventListener("mousemove", onHover, true);
      doc.addEventListener("click", onPlace, true);
      doc.addEventListener("keydown", onPlaceKey, true);
      announce("Click the spot you want to comment on, or press Escape to cancel.");
    }

    function onHover(event) {
      if (event.composedPath().includes(host)) return;
      const rect = event.target.getBoundingClientRect?.();
      if (!rect) return;
      highlight.hidden = false;
      highlight.style.setProperty("left", `${rect.left}px`);
      highlight.style.setProperty("top", `${rect.top}px`);
      highlight.style.setProperty("width", `${rect.width}px`);
      highlight.style.setProperty("height", `${rect.height}px`);
    }

    function onPlace(event) {
      if (event.composedPath().includes(host)) return;
      event.preventDefault();
      event.stopPropagation();
      compose(
        anchorFor(event.target, event.clientX, event.clientY, win),
        event.clientX,
        event.clientY,
      );
    }

    function onPlaceKey(event) {
      if (event.key === "Escape") {
        event.preventDefault();
        stopPlacing();
        toggle.focus();
      }
    }

    // ----- popovers -----

    let popover = null;
    let returnFocus = null;
    function closePopover() {
      if (!popover) return;
      popover.remove();
      popover = null;
      state.open = null;
      renderPins();
      returnFocus?.focus?.();
      returnFocus = null;
    }

    function place(node, x, y) {
      const left = Math.min(Math.max(8, x + 12), Math.max(8, win.innerWidth - 336));
      const top = Math.min(Math.max(8, y - 12), Math.max(8, win.innerHeight - 280));
      node.style.setProperty("left", `${Math.round(left)}px`);
      node.style.setProperty("top", `${Math.round(top)}px`);
    }

    function nameField() {
      if (state.me.verified) return null;
      return el(
        "label",
        { class: "field" },
        el("span", { text: "Your name" }),
        el("input", {
          name: "author",
          maxlength: MAX_NAME,
          autocomplete: "name",
          required: true,
          value: storedName(),
        }),
      );
    }

    function form(label, submitText, onSubmit) {
      const status = el("p", { class: "error", role: "alert" });
      const node = el(
        "form",
        {
          onsubmit: async (event) => {
            event.preventDefault();
            const data = new FormData(node);
            const body = String(data.get("body") || "").trim();
            const author = cleanName(data.get("author") ?? storedName());
            if (!body) return;
            if (!state.me.verified && !author) {
              status.textContent = "Please add your name.";
              return;
            }
            if (!state.me.verified) rememberName(author);
            const submit = node.querySelector("button[type=submit]");
            submit.disabled = true;
            submit.setAttribute("aria-busy", "true");
            try {
              await onSubmit(body, author);
            } catch (error) {
              status.textContent = error.message;
              submit.disabled = false;
              submit.removeAttribute("aria-busy");
            }
          },
        },
        nameField(),
        el(
          "label",
          { class: "field" },
          el("span", { class: "sr", text: label }),
          el("textarea", {
            name: "body",
            rows: 3,
            maxlength: MAX_BODY,
            required: true,
            placeholder: label,
            onkeydown: (event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) node.requestSubmit();
            },
          }),
        ),
        status,
        el(
          "div",
          { class: "actions" },
          el("button", {
            type: "button",
            class: "secondary",
            onclick: closePopover,
            text: "Cancel",
          }),
          el("button", { type: "submit", class: "primary", text: submitText }),
        ),
      );
      return node;
    }

    function openPopover(node, x, y, focusFrom) {
      closePopover();
      returnFocus = focusFrom || doc.activeElement;
      popover = node;
      place(node, x, y);
      node.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          closePopover();
        }
      });
      shadow.append(node);
      const author = node.querySelector("input[name=author]");
      (author && !author.value
        ? author
        : node.querySelector("textarea") || node.querySelector("button")
      )?.focus();
    }

    function compose(anchor, x, y) {
      stopPlacing();
      const node = el(
        "div",
        { class: "popover", role: "dialog", "aria-label": "New comment" },
        form("Add a comment", "Comment", async (body, author) => {
          const thread = await call("POST", "threads", {
            path: win.location.pathname,
            anchor,
            body,
            author,
          });
          upsert(thread);
          closePopover();
          render();
          announce("Comment posted.");
        }),
      );
      openPopover(node, x, y, toggle);
    }

    function openThread(id, from) {
      const thread = state.threads.find((t) => t.id === id);
      if (!thread) return;
      const point = resolveAnchor(thread.anchor, doc, win) || {
        x: win.innerWidth / 2,
        y: win.innerHeight / 3,
      };
      const resolve = el("button", {
        type: "button",
        class: "secondary",
        text: thread.resolved ? "Reopen" : "Resolve",
        onclick: async () => {
          resolve.disabled = true;
          try {
            const updated = await call("POST", `threads/${encodeURIComponent(id)}/resolve`, {
              resolved: !thread.resolved,
              author: cleanName(storedName()),
            });
            upsert(updated);
            closePopover();
            render();
            announce(updated.resolved ? "Resolved." : "Reopened.");
          } catch (error) {
            resolve.disabled = false;
            announce(error.message);
          }
        },
      });
      const node = el(
        "div",
        { class: "popover", role: "dialog", "aria-label": "Comment thread" },
        el("ol", { class: "comments" }, ...thread.comments.map(commentView)),
        el("div", { class: "row-actions" }, resolve),
        form("Reply", "Reply", async (body, author) => {
          const updated = await call("POST", `threads/${encodeURIComponent(id)}/replies`, {
            body,
            author,
          });
          upsert(updated);
          render();
          openThread(id, from);
          announce("Reply posted.");
        }),
      );
      state.open = id;
      renderPins();
      openPopover(node, point.x, point.y, from);
      if (point.exact === false || point.y < 0 || point.y > win.innerHeight) {
        win.scrollTo({
          top: Math.max(0, (thread.anchor?.top ?? 0) - win.innerHeight / 3),
          behavior: reduceMotion ? "auto" : "smooth",
        });
      }
    }

    // ----- lifecycle -----

    let frame = 0;
    const reposition = () => {
      if (frame) return;
      frame = win.requestAnimationFrame(() => {
        frame = 0;
        renderPins();
      });
    };
    win.addEventListener("scroll", reposition, { passive: true });
    win.addEventListener("resize", reposition, { passive: true });

    const poll = win.setInterval(() => {
      if (doc.visibilityState === "visible" && !popover) load();
    }, POLL_MS);

    const ready = load().then(() => {
      const target = hashTarget(win.location.hash);
      if (target) openThread(target);
    });

    win.__teitunnelComments = { reload: load };
    return {
      ready,
      shadow,
      host,
      state,
      destroy() {
        win.clearInterval(poll);
        stopPlacing();
        host.remove();
        delete win.__teitunnelComments;
      },
    };
  }

  function applyStyles(shadow, doc) {
    const css = `
:host { all: initial; }
* { box-sizing: border-box; }
.layer, .highlight { position: fixed; inset: 0; pointer-events: none; }
.highlight { inset: auto; outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
:host, .panel, .popover, .hint, .fab {
  --bg: #ffffff; --fg: #1d1d1f; --muted: #6e6e73; --line: rgba(0,0,0,.12); --accent: #0a66d8; --accent-fg: #fff; --field: #f5f5f7; --danger: #c4001a;
  font: 13px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; color: var(--fg);
}
@media (prefers-color-scheme: dark) {
  :host, .panel, .popover, .hint, .fab {
    --bg: #2c2c2e; --fg: #f5f5f7; --muted: #a1a1a6; --line: rgba(255,255,255,.14); --accent: #409cff; --accent-fg: #0b0b0c; --field: #1c1c1e; --danger: #ff6961;
  }
}
button { font: inherit; color: inherit; cursor: default; }
button:focus-visible, input:focus-visible, textarea:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.fab { pointer-events: auto; position: fixed; right: 16px; bottom: 16px; display: inline-flex; align-items: center; gap: 6px; height: 36px; min-width: 36px; padding: 0 12px; border-radius: 18px; border: 1px solid var(--line); background: var(--bg); box-shadow: 0 4px 14px rgba(0,0,0,.18); }
.fab svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.4; }
.count:empty { display: none; }
.count { font-weight: 600; font-variant-numeric: tabular-nums; }
.pin { pointer-events: auto; position: fixed; width: 24px; height: 24px; margin: -24px 0 0 0; border-radius: 12px 12px 12px 2px; border: 2px solid var(--bg); background: var(--accent); color: var(--accent-fg); font-weight: 600; font-size: 11px; box-shadow: 0 2px 6px rgba(0,0,0,.3); transition: transform .15s ease; }
.pin:hover, .pin.active { transform: scale(1.12); }
.pin.resolved { background: var(--muted); }
.panel, .popover { pointer-events: auto; position: fixed; background: var(--bg); border: 1px solid var(--line); border-radius: 12px; box-shadow: 0 12px 40px rgba(0,0,0,.25); }
.panel { right: 16px; bottom: 60px; width: min(340px, calc(100vw - 32px)); max-height: min(520px, calc(100vh - 80px)); overflow: auto; padding: 12px; display: grid; gap: 10px; }
.panel[hidden], .hint[hidden], .highlight[hidden] { display: none; }
.panel header { display: flex; align-items: center; justify-content: space-between; }
h2 { font-size: 13px; font-weight: 600; margin: 0; }
.icon { width: 24px; height: 24px; border: 0; background: none; border-radius: 6px; display: grid; place-items: center; }
.icon svg { width: 12px; height: 12px; stroke: currentColor; stroke-width: 1.6; }
.threads { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
.row { width: 100%; text-align: left; border: 0; background: none; border-radius: 8px; padding: 6px; display: grid; grid-template-columns: 22px 1fr; column-gap: 8px; }
.row:hover { background: var(--field); }
.num { grid-row: span 2; width: 20px; height: 20px; border-radius: 10px; background: var(--accent); color: var(--accent-fg); font-size: 11px; font-weight: 600; display: grid; place-items: center; }
.num.resolved { background: var(--muted); }
.excerpt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sub, .empty, time { color: var(--muted); font-size: 12px; }
.check { display: flex; gap: 6px; align-items: center; color: var(--muted); }
.popover { width: min(320px, calc(100vw - 16px)); max-height: calc(100vh - 16px); overflow: auto; padding: 12px; display: grid; gap: 10px; }
.comments { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
.meta { display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; }
.badge { font-size: 11px; color: var(--muted); border: 1px solid var(--line); border-radius: 4px; padding: 0 4px; }
.body { margin: 2px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; }
form { display: grid; gap: 8px; }
.field { display: grid; gap: 4px; }
.field span { color: var(--muted); font-size: 12px; }
input, textarea { font: inherit; color: var(--fg); background: var(--field); border: 1px solid var(--line); border-radius: 6px; padding: 6px 8px; width: 100%; resize: vertical; }
.actions, .row-actions { display: flex; justify-content: flex-end; gap: 8px; }
.primary, .secondary { height: 28px; padding: 0 12px; border-radius: 6px; border: 1px solid var(--line); background: var(--field); }
.primary { background: var(--accent); color: var(--accent-fg); border-color: transparent; font-weight: 500; }
button:disabled { opacity: .5; }
.hint { pointer-events: auto; position: fixed; left: 50%; top: 16px; transform: translateX(-50%); display: flex; gap: 12px; align-items: center; background: var(--bg); border: 1px solid var(--line); border-radius: 10px; padding: 8px 12px; box-shadow: 0 8px 24px rgba(0,0,0,.2); }
.link { border: 0; background: none; color: var(--accent); padding: 0; }
.error { color: var(--danger); margin: 0; font-size: 12px; }
.error:empty { display: none; }
.sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
@media (prefers-reduced-motion: reduce) { .pin { transition: none; } }
@media (forced-colors: active) { .pin, .num, .primary { border: 1px solid CanvasText; } }
`;
    const Sheet = doc.defaultView?.CSSStyleSheet;
    if (Sheet && "adoptedStyleSheets" in shadow) {
      try {
        const sheet = new Sheet();
        sheet.replaceSync(css);
        shadow.adoptedStyleSheets = [sheet];
        return;
      } catch {
        // Fall through to a <style> element.
      }
    }
    const style = doc.createElement("style");
    style.textContent = css;
    shadow.append(style);
  }
})();
