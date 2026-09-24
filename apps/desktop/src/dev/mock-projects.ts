import type { ExposureReport, ProjectEntry, ProjectStatus } from "@/lib/ipc/bindings";

/**
 * Dev-only project and exposure-check fixtures for the browser preview (see
 * `mock-ipc.ts`): a shop project with a route applied, one to add, a share and a skipped
 * local domain, and a service leaking its `.env` file.
 */
const core = (key: string, args: Record<string, string | number> = {}) => ({
  key: `core.${key}`,
  args,
});

const PATH = "/Users/ada/code/shop/teitunnel.yml";

const projects: ProjectEntry[] = [
  { path: PATH, name: "shop", addedAt: 1, appliedAt: Date.now() - 86_400_000, createdRoutes: [] },
  {
    path: "/Users/ada/code/docs/teitunnel.yml",
    name: "docs",
    addedAt: 2,
    appliedAt: null,
    createdRoutes: [],
  },
];

const status: ProjectStatus = {
  path: PATH,
  name: "shop",
  modifiedAt: 1,
  problem: null,
  diagnostics: [
    {
      line: 24,
      column: 1,
      severity: "warning",
      message: core("project.unknownKey", { key: "protection" }),
    },
  ],
  plan: {
    path: PATH,
    name: "shop",
    accountId: "a1",
    fingerprint: "fp",
    requiresConfirmation: false,
    items: [
      {
        kind: "route",
        name: "shop.acme.dev",
        target: "3000",
        state: "applied",
        line: 6,
        note: null,
      },
      {
        kind: "route",
        name: "api.acme.dev ^/v1",
        target: "http://localhost:4000",
        state: "missing",
        line: 8,
        note: null,
      },
      {
        kind: "share",
        name: "feat-login-shop.acme.dev",
        target: "http://localhost:5173",
        state: "missing",
        line: 14,
        note: null,
      },
      {
        kind: "snapshot",
        name: "docs",
        target: "dist",
        state: "applied",
        line: 18,
        note: null,
      },
      {
        kind: "localDomain",
        name: "shop.localhost",
        target: "localhost:3000",
        state: "missing",
        line: 22,
        note: null,
      },
    ],
    routes: [
      {
        hostname: "api.acme.dev",
        path: "^/v1",
        tunnelId: null,
        change: {
          type: "addRoute",
          route: {
            hostname: "api.acme.dev",
            path: "^/v1",
            origin: "http://localhost:4000",
            access: null,
            options: null,
          },
        },
        plan: {
          fingerprint: "r1",
          requiresConfirmation: false,
          warnings: [],
          steps: [
            {
              kind: "putConfig",
              description: core("plan.step.putConfig", { count: 2, tunnel: "Ada's MacBook" }),
              command: null,
            },
            {
              kind: "createRecord",
              description: core("plan.step.createRecord", {
                hostname: "api.acme.dev",
                tunnel: "Ada's MacBook",
              }),
              command: null,
            },
          ],
        },
      },
    ],
    shares: [
      {
        origin: "http://localhost:5173",
        hostname: "feat-login-shop.acme.dev",
        expiresAfter: 7200,
        hostHeader: { mode: "set", value: "localhost:5173" },
        login: null,
        inspect: true,
      },
    ],
    snapshots: [
      {
        name: "docs",
        source: { type: "folder", path: "/Users/ada/code/shop/dist" },
        hostname: "docs.acme.dev",
        exists: true,
      },
    ],
    localDomains: [{ name: "shop.localhost", port: 3000, wildcard: false, exists: false }],
  },
};

const leak: ExposureReport = {
  origin: "http://localhost:4000",
  requests: 21,
  incomplete: false,
  elapsedMs: 140,
  findings: [
    {
      kind: "envFile",
      severity: "high",
      path: "/.env",
      title: core("exposure.envFile.title"),
      advice: core("exposure.envFile.advice"),
      detail: "APP_KEY, DATABASE_URL, STRIPE_SECRET_KEY",
    },
    {
      kind: "djangoDebug",
      severity: "medium",
      path: "/teitunnel-exposure-check-404",
      title: core("exposure.djangoDebug.title"),
      advice: core("exposure.djangoDebug.advice"),
      detail: null,
    },
  ],
};

/** Answers the project and exposure commands; `undefined` for anything else. */
export function projectsMock(cmd: string, payload: Record<string, unknown>): unknown {
  switch (cmd) {
    case "projects_list":
      return projects;
    case "projects_status":
      return payload["path"] === PATH ? status : { ...status, path: payload["path"], plan: null };
    case "projects_modified":
      return 1;
    case "exposure_check":
      return payload["origin"] === "http://localhost:4000" ? leak : null;
    default:
      return undefined;
  }
}
