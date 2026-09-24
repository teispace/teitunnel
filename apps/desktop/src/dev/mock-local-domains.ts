import type {
  LocalDomainInput,
  LocalDomainsStatus,
  LocalDomainView,
  TrustView,
} from "@/lib/ipc/bindings";

/** Dev-only fixtures for Local Domains (WebKit screenshots, design review). */
const domain = (view: Partial<LocalDomainView> & { name: string }): LocalDomainView => ({
  url: `https://${view.name}`,
  origin: "http://localhost:3000",
  target: { kind: "port", port: 3000 },
  wildcard: false,
  https: true,
  inspect: false,
  project: null,
  createdAt: 1_790_000_000,
  serving: true,
  resolution: "ok",
  tapId: null,
  requests: 0,
  ...view,
});

let domains: LocalDomainView[] = [
  domain({
    name: "api.localhost",
    origin: "http://localhost:4000",
    target: { kind: "port", port: 4000 },
    wildcard: true,
    inspect: true,
    requests: 128,
  }),
  domain({
    name: "phone.local",
    origin: "http://localhost:5173",
    target: { kind: "port", port: 5173 },
    requests: 12,
  }),
  domain({
    name: "shop.test",
    project: "/Users/ada/code/shop/teitunnel.yml",
    requests: 342,
  }),
];
let trusted = false;
let lan = false;

const ca = {
  commonName: "Teitunnel Local CA (ada@studio)",
  sha256: "4F2A…",
  notAfter: 2_100_000_000,
};

function status(): LocalDomainsStatus {
  return {
    running: true,
    httpsPort: 443,
    httpPort: 80,
    portProblems: [],
    lan,
    lanAddresses: ["192.168.1.24"],
    resolver: {
      needed: true,
      responding: true,
      port: 53535,
      configured: true,
      error: null,
      setup: [
        {
          command:
            "sudo mkdir -p /etc/resolver\nprintf '%s' 'nameserver 127.0.0.1\nport 53535\n' | sudo tee /etc/resolver/test > /dev/null",
        },
      ],
      teardown: [{ command: "sudo rm -f /etc/resolver/test" }],
    },
    ca,
    domains,
    error: null,
    platform: "macos",
  };
}

function trust(): TrustView {
  return {
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
    ca,
    platform: "macos",
  };
}

/** Answers the local domains commands; `undefined` for anything else. */
export function localDomainsMock(cmd: string, payload: Record<string, unknown>): unknown {
  switch (cmd) {
    case "local_domains_status":
      return status();
    case "local_domains_trust_status":
      return trust();
    case "local_domains_trust":
      trusted = true;
      return trust();
    case "local_domains_untrust":
      trusted = false;
      return trust();
    case "local_domains_set_lan":
      lan = payload["lan"] === true;
      return null;
    case "local_domains_add": {
      const input = payload["input"] as LocalDomainInput;
      const added = domain({ name: input.name, https: input.https, inspect: input.inspect });
      domains = [...domains, added].sort((a, b) => a.name.localeCompare(b.name));
      return added;
    }
    case "local_domains_remove":
      domains = domains.filter((d) => d.name !== payload["name"]);
      return null;
    case "local_domains_set_inspect": {
      domains = domains.map((d) =>
        d.name === payload["name"] ? { ...d, inspect: payload["inspect"] === true } : d,
      );
      return domains.find((d) => d.name === payload["name"]);
    }
    default:
      return undefined;
  }
}
