// Teitunnel's server dashboard. Every element is built with textContent (never
// innerHTML with data), and every change is a reviewed plan: preview, then apply the
// exact plan by its fingerprint.

const $ = (id) => document.getElementById(id);

async function api(path, body) {
  const response = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    headers: { "content-type": "application/json", "x-teitunnel": "1" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const data = await response.json().catch(() => ({}));
  if (response.status === 401 && path !== "/api/login") {
    show(false);
  }
  if (!response.ok) throw new Error(data.error || `Request failed (${response.status})`);
  return data;
}

function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else if (value !== undefined && value !== null && value !== false)
      node.setAttribute(key, value === true ? "" : value);
  }
  for (const child of children) if (child) node.append(child);
  return node;
}

const HEALTH = {
  live: ["live", "Live"],
  noDns: ["warn", "No DNS record"],
  dnsElsewhere: ["warn", "DNS points elsewhere"],
  connecting: ["warn", "Connecting"],
  restarting: ["warn", "Restarting"],
  connectionLost: ["warn", "Connection lost"],
  keepsStopping: ["bad", "Keeps stopping"],
  stopped: ["", "Stopped"],
};

function routeHealth(route, tunnels) {
  if (route.dns.state === "missing") return HEALTH.noDns;
  if (route.dns.state === "elsewhere") return HEALTH.dnsElsewhere;
  const tunnel = tunnels.find((t) => t.id === route.tunnelId);
  switch (tunnel?.connector?.state) {
    case "healthy":
      return HEALTH.live;
    case "starting":
    case "connecting":
      return HEALTH.connecting;
    case "crashed":
      return HEALTH.restarting;
    case "degraded":
      return HEALTH.connectionLost;
    case "crashLoop":
      return HEALTH.keepsStopping;
    default:
      return HEALTH.stopped;
  }
}

// ── Reviewed changes ──────────────────────────────────────────────────────────

function text(value) {
  // The server renders every message as English text.
  return typeof value === "string" ? value : JSON.stringify(value);
}

async function review(accountId, tunnelId, change, title) {
  const dialog = $("review");
  $("review-title").textContent = title;
  $("review-steps").replaceChildren();
  $("review-warnings").replaceChildren();
  $("review-error").textContent = "";
  $("review-confirm").checked = false;
  let plan;
  try {
    plan = await api("/api/preview", { accountId, tunnelId, change });
  } catch (error) {
    alertError(error);
    return;
  }
  if (plan.steps.length === 0) {
    $("review-steps").append(el("li", { text: "Nothing to change: it's already set up." }));
  }
  for (const step of plan.steps)
    $("review-steps").append(el("li", { text: text(step.description) }));
  for (const warning of plan.warnings) {
    $("review-warnings").append(el("li", { text: describeWarning(warning) }));
  }
  $("review-confirm-row").hidden = !plan.requiresConfirmation;
  $("review-apply").disabled = plan.steps.length === 0;
  dialog.returnValue = "";
  dialog.showModal();
  dialog.onclose = async () => {
    if (dialog.returnValue !== "apply") return;
    try {
      const outcome = await api("/api/apply", {
        accountId,
        tunnelId,
        change,
        fingerprint: plan.fingerprint,
        confirmed: $("review-confirm").checked,
      });
      if (outcome.type !== "applied") alertError(new Error(text(outcome.error)));
    } catch (error) {
      alertError(error);
    }
    refresh();
  };
}

function describeWarning(warning) {
  switch (warning.type) {
    case "replacesForeignRecord":
      return `${warning.hostname} has a ${warning.kind} record (${warning.content}) Teitunnel didn't create. It will be replaced.`;
    case "deletesForeignRecord":
      return `The ${warning.kind} record for ${warning.hostname} wasn't created by Teitunnel. It will be deleted.`;
    case "remoteOrigin":
      return `${warning.origin} isn't on this machine. It must be reachable from here.`;
    case "tunnelEmpty":
      return "No routes will be left on the tunnel.";
    default:
      return warning.type;
  }
}

function alertError(error) {
  const banner = el("div", { class: "card error", role: "alert", text: error.message });
  $("dashboard").prepend(banner);
  setTimeout(() => banner.remove(), 8000);
}

// ── Rendering ────────────────────────────────────────────────────────────────

function accountCard(account) {
  const card = el("section", { class: "card" }, el("h2", { text: account.name }));
  if (!account.overview) {
    card.append(el("p", { class: "error", text: account.error ?? "Couldn't read this account." }));
    return card;
  }
  const { tunnels, routes, zones } = account.overview;

  card.append(el("h3", { text: "Tunnels on this machine" }));
  const tunnelList = el("ul", { class: "list" });
  for (const tunnel of tunnels) {
    const running = tunnel.connector && tunnel.connector.state !== "stopped";
    tunnelList.append(
      el(
        "li",
        {},
        el("span", { class: `dot ${running ? "live" : ""}` }),
        el("span", { class: "grow", text: tunnel.name }),
        tunnel.isDefault ? el("span", { class: "badge", text: "Default" }) : null,
        el("button", {
          class: "danger",
          text: "Delete…",
          onclick: () =>
            review(
              account.id,
              tunnel.id,
              { type: "removeTunnel" },
              `Delete tunnel “${tunnel.name}”`,
            ),
        }),
      ),
    );
  }
  if (tunnels.length === 0)
    tunnelList.append(el("li", { class: "muted", text: "None yet: adding a route creates one." }));
  card.append(tunnelList);
  const tunnelName = el("input", {
    placeholder: "staging",
    "aria-label": "New tunnel name",
    maxlength: "64",
  });
  card.append(
    el(
      "form",
      {
        class: "row",
        onsubmit: (event) => {
          event.preventDefault();
          review(account.id, null, { type: "createTunnel", name: tunnelName.value }, "New tunnel");
        },
      },
      tunnelName,
      el("span"),
      el("span"),
      el("button", { type: "submit", text: "New Tunnel" }),
    ),
  );

  card.append(el("h3", { text: "Routes" }));
  const list = el("ul", { class: "list" });
  for (const route of routes) {
    const [dot, label] = routeHealth(route, tunnels);
    const tunnel = tunnels.find((t) => t.id === route.tunnelId);
    list.append(
      el(
        "li",
        {},
        el("span", { class: `dot ${dot}`, title: label, role: "img", "aria-label": label }),
        el(
          "span",
          { class: "grow" },
          el("a", {
            href: `https://${route.hostname}`,
            target: "_blank",
            rel: "noopener noreferrer",
            text: route.hostname + (route.path ?? ""),
          }),
          el("span", { class: "muted", text: ` → ${route.origin}` }),
        ),
        tunnels.length > 1 && tunnel ? el("span", { class: "badge", text: tunnel.name }) : null,
        route.access ? el("span", { class: "badge", text: "Login" }) : null,
        route.temporary ? el("span", { class: "badge", text: "Temporary" }) : null,
        el("button", {
          class: "danger",
          text: "Remove…",
          onclick: () =>
            review(
              account.id,
              route.tunnelId,
              { type: "removeRoute", hostname: route.hostname, path: route.path },
              `Remove ${route.hostname}`,
            ),
        }),
      ),
    );
  }
  if (routes.length === 0) list.append(el("li", { class: "muted", text: "No routes yet." }));
  card.append(list);

  const hostname = el("input", {
    id: `host-${account.id}`,
    placeholder: `app.${zones[0]?.name ?? "example.com"}`,
    required: true,
  });
  const origin = el("input", {
    id: `origin-${account.id}`,
    placeholder: "3000 or http://web:80",
    required: true,
  });
  const tunnelSelect = el("select", { id: `tunnel-${account.id}` });
  for (const tunnel of tunnels) {
    tunnelSelect.append(
      el("option", { value: tunnel.isDefault ? "" : tunnel.id, text: tunnel.name }),
    );
  }
  if (tunnels.length === 0) tunnelSelect.append(el("option", { value: "", text: "New tunnel" }));
  card.append(
    el(
      "form",
      {
        class: "row",
        onsubmit: (event) => {
          event.preventDefault();
          review(
            account.id,
            tunnelSelect.value || null,
            {
              type: "addRoute",
              route: {
                hostname: hostname.value.trim(),
                origin: origin.value.trim(),
                path: null,
                access: null,
              },
            },
            `Add ${hostname.value.trim()}`,
          );
        },
      },
      el("div", {}, el("label", { for: hostname.id, text: "Public hostname" }), hostname),
      el("div", {}, el("label", { for: origin.id, text: "Service" }), origin),
      el("div", {}, el("label", { for: tunnelSelect.id, text: "Tunnel" }), tunnelSelect),
      el("button", { type: "submit", class: "primary", text: "Add Route…" }),
    ),
  );
  return card;
}

async function refresh() {
  let data;
  try {
    data = await api("/api/overview");
  } catch {
    return;
  }
  $("machine").textContent = data.machine;
  $("accounts").replaceChildren(...data.accounts.map(accountCard));
  const shares = $("shares");
  shares.replaceChildren(
    ...data.shares.map((share) =>
      el(
        "li",
        {},
        el("a", {
          class: "grow",
          href: `https://${share.hostname}`,
          target: "_blank",
          rel: "noopener noreferrer",
          text: share.hostname,
        }),
        el("span", { class: "muted", text: share.origin }),
      ),
    ),
  );
  if (data.shares.length === 0) shares.append(el("li", { class: "muted", text: "None running." }));
}

// ── Session ──────────────────────────────────────────────────────────────────

let timer = null;

function show(signedIn) {
  $("signin").hidden = signedIn;
  $("dashboard").hidden = !signedIn;
  $("signout").hidden = !signedIn;
  clearInterval(timer);
  if (signedIn) {
    refresh();
    timer = setInterval(refresh, 10000);
  } else {
    $("password").focus();
  }
}

$("signin-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  $("signin-error").textContent = "";
  try {
    await api("/api/login", { password: $("password").value });
    $("password").value = "";
    show(true);
  } catch (error) {
    $("signin-error").textContent = error.message;
  }
});

$("signout").addEventListener("click", async () => {
  await api("/api/logout", {}).catch(() => {});
  show(false);
});

api("/api/session")
  .then((session) => show(session.signedIn))
  .catch(() => show(false));
