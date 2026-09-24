import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import type {
  LocalDomainsStatus,
  LocalDomainView,
  NameResolution,
  TrustView,
} from "@/lib/ipc/bindings";
import { LocalDomainsPage } from "./local-domains-page";
import { completeName, domainState, suffixOf } from "./model";

const view = (name: string, extra: Partial<LocalDomainView> = {}): LocalDomainView => ({
  name,
  url: `https://${name}`,
  origin: "http://localhost:3000",
  target: { kind: "port", port: 3000 },
  wildcard: false,
  https: true,
  inspect: false,
  project: null,
  createdAt: 1,
  serving: true,
  resolution: "ok",
  tapId: null,
  requests: 3,
  ...extra,
});

let domains: LocalDomainView[];
let trusted: boolean;
let resolverError: boolean;
let calls: { cmd: string; args: Record<string, unknown> }[];

const status = (): LocalDomainsStatus => ({
  running: domains.length > 0,
  httpsPort: 443,
  httpPort: 80,
  portProblems: [],
  lan: false,
  lanAddresses: ["192.168.1.24"],
  resolver: {
    needed: domains.some((d) => d.name.endsWith(".test")),
    responding: true,
    port: 53535,
    configured: false,
    error: resolverError
      ? { key: "core.localDomains.error.dns", args: { detail: "port 53 is in use" } }
      : null,
    setup: [{ command: "sudo tee /etc/resolver/test" }],
    teardown: [],
  },
  ca: null,
  domains,
  error: null,
  platform: "macos",
});

const trust = (): TrustView => ({
  trusted,
  stores: [
    {
      kind: "macosKeychain",
      path: null,
      flavor: null,
      state: { state: trusted ? "trusted" : "absent" },
    },
  ],
  steps: [],
  ca: null,
  platform: "macos",
});

beforeEach(() => {
  domains = [view("api.localhost", { inspect: true }), view("shop.test")];
  trusted = false;
  resolverError = false;
  calls = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "local_domains_status":
        return status();
      case "local_domains_trust_status":
        return trust();
      case "local_domains_trust":
        trusted = true;
        return trust();
      case "local_domains_add": {
        const input = payload["input"] as { name: string };
        if (input.name === "taken.localhost") {
          throw {
            code: "invalidInput",
            message: {
              key: "core.raw",
              args: { text: "taken.localhost is already a local domain." },
            },
            hint: null,
            field: "name",
          };
        }
        const added = view(input.name);
        domains = [...domains, added];
        return added;
      }
      case "local_domains_remove":
        domains = domains.filter((d) => d.name !== payload["name"]);
        return null;
      case "local_domains_set_inspect":
        domains = domains.map((d) =>
          d.name === payload["name"] ? { ...d, inspect: payload["inspect"] === true } : d,
        );
        return domains.find((d) => d.name === payload["name"]);
      case "services_list":
        return [];
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <LocalDomainsPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("LocalDomainsPage", () => {
  it("lists domains with their state and offers to set up trust in place", async () => {
    renderPage();
    expect(await screen.findByRole("option", { name: /api\.localhost/ })).toBeTruthy();
    expect(screen.getByRole("option", { name: /shop\.test/ })).toBeTruthy();
    const callout = await screen.findByRole("region", {
      name: "Browsers don't trust your local domains yet",
    });
    fireEvent.click(within(callout).getByRole("button", { name: "Set Up Trust…" }));
    const sheet = await screen.findByRole("dialog", { name: "Trust Local Domains" });
    expect(within(sheet).getByText(/login keychain as trusted/)).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: "Trust" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "local_domains_trust")).toBe(true));
    expect(calls.find((c) => c.cmd === "local_domains_trust")?.args).toEqual({
      options: { browsers: true, firefoxSystemRoots: false },
    });
    expect(await within(sheet).findByText("Browsers trust your local domains.")).toBeTruthy();
    expect(within(sheet).getByRole("button", { name: "Done" })).toBeTruthy();
  });

  it("shows the selected domain's address, service and inspection", async () => {
    renderPage();
    expect(await screen.findByRole("heading", { name: "api.localhost" })).toBeTruthy();
    expect(screen.getByText("https://api.localhost")).toBeTruthy();
    expect(screen.getByText("localhost:3000")).toBeTruthy();
    expect(screen.getByText("3 requests since it started")).toBeTruthy();
    const record = screen.getByRole("switch", { name: "Record requests" });
    expect(record.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(record);
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "local_domains_set_inspect")?.args).toEqual({
        name: "api.localhost",
        inspect: false,
      }),
    );
    // Phones need a .local name.
    expect(screen.getByText(/Add a \.local name/)).toBeTruthy();
  });

  it("adds a domain, completing the name with .localhost", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Add Local Domain" }));
    const sheet = await screen.findByRole("dialog", { name: "Add Local Domain" });
    fireEvent.change(within(sheet).getByRole("textbox", { name: "Name" }), {
      target: { value: "Blog" },
    });
    expect(within(sheet).getByText("It will be blog.localhost.")).toBeTruthy();
    fireEvent.change(within(sheet).getByRole("combobox"), { target: { value: "4000" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Add" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "local_domains_add")?.args).toEqual({
        input: {
          name: "blog.localhost",
          target: "4000",
          wildcard: false,
          https: true,
          inspect: false,
        },
      }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(await screen.findByRole("option", { name: /blog\.localhost/ })).toBeTruthy();
  });

  it("shows why a name was refused next to the name", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Add Local Domain" }));
    const sheet = await screen.findByRole("dialog", { name: "Add Local Domain" });
    fireEvent.change(within(sheet).getByRole("textbox", { name: "Name" }), {
      target: { value: "taken.localhost" },
    });
    fireEvent.change(within(sheet).getByRole("combobox"), { target: { value: "3000" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Add" }));
    expect(
      await within(sheet).findByText("taken.localhost is already a local domain."),
    ).toBeTruthy();
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  it("walks through .test names in place until they resolve", async () => {
    domains = [view("shop.test", { resolution: "needsResolver" })];
    renderPage();
    const fix = await screen.findByRole("region", { name: "Set up .test names once" });
    expect(within(fix).getByText("sudo tee /etc/resolver/test")).toBeTruthy();
    expect(within(fix).getByText(/Checking again every few seconds/)).toBeTruthy();
    domains = [view("shop.test", { resolution: "ok" })];
    await waitFor(
      () => expect(screen.queryByRole("region", { name: "Set up .test names once" })).toBeNull(),
      { timeout: 5000 },
    );
  });

  it("offers to restart when the .test name server couldn't start", async () => {
    resolverError = true;
    renderPage();
    const fix = await screen.findByRole("region", {
      name: "The name server for .test names isn't running",
    });
    expect(within(fix).getByText(/port 53 is in use/)).toBeTruthy();
    fireEvent.click(within(fix).getByRole("button", { name: "Try Again" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "local_domains_restart")).toBe(true));
  });

  it("removes a domain after confirming", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Remove Local Domain…" }));
    const alert = await screen.findByRole("dialog", { name: "Remove api.localhost?" });
    fireEvent.click(within(alert).getByRole("button", { name: "Remove" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "local_domains_remove")?.args).toEqual({
        name: "api.localhost",
      }),
    );
  });

  it("starts empty with one action", async () => {
    domains = [];
    renderPage();
    expect(await screen.findByText("No local domains")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Add Local Domain" }).length).toBeGreaterThan(0);
  });
});

describe("model", () => {
  it("completes names and reads suffixes", () => {
    expect(completeName(" Shop ")).toBe("shop.localhost");
    expect(completeName("shop.test.")).toBe("shop.test");
    expect(completeName("x.local")).toBe("x.local");
    expect(completeName("")).toBe("");
    expect(suffixOf("a.local")).toBe("local");
    expect(suffixOf("a.com")).toBeNull();
  });

  it("puts the most urgent state first", () => {
    const at = (resolution: NameResolution, serving = true) =>
      domainState(view("a.test", { resolution, serving }), false).dot;
    expect(at("ok", false)).toBe("error");
    expect(at("needsResolver")).toBe("warning");
    expect(domainState(view("a.localhost"), true).dot).toBe("healthy");
    expect(domainState(view("a.localhost"), false).label).toBe("Not trusted by browsers yet");
    expect(domainState(view("a.localhost", { https: false }), false).dot).toBe("healthy");
  });
});
