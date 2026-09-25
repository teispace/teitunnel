// Mock answers for comments, offline pages and webhook inboxes (dev mode, screenshots).
import type {
  FrontView,
  InboxItem,
  Outcome,
  PlanView,
  SubjectView,
  TapView,
  Thread_Serialize as Thread,
} from "@/lib/ipc/bindings";

const now = Date.now();
const minutes = (n: number) => now - n * 60_000;

const subjects: SubjectView[] = [
  {
    key: "snapshot:s1",
    kind: "snapshot",
    accountId: "acc-personal",
    label: "Launch",
    url: "https://preview.teispace.com",
    open: 2,
    comments: 5,
    unread: 2,
    latestAt: minutes(4),
  },
  {
    key: "route:acc-personal:app.teispace.com",
    kind: "route",
    accountId: "acc-personal",
    label: "app.teispace.com",
    url: "https://app.teispace.com",
    open: 1,
    comments: 1,
    unread: 0,
    latestAt: minutes(95),
  },
];

const threads: Thread[] = [
  {
    id: "c1",
    path: "/pricing",
    anchor: {
      selector: "main > h1:nth-of-type(1)",
      x: 0.4,
      y: 0.5,
      left: 320,
      top: 180,
      vw: 1440,
      vh: 900,
    },
    resolved: false,
    resolvedBy: null,
    resolvedAt: null,
    createdAt: minutes(42),
    comments: [
      {
        id: "c1",
        author: "Teispace Design",
        email: null,
        verified: false,
        byOwner: false,
        body: "The yearly price should show the discount next to it, like on the landing page.",
        createdAt: minutes(42),
      },
      {
        id: "c2",
        author: "Teispace",
        email: null,
        verified: false,
        byOwner: true,
        body: "Good catch, adding it now.",
        createdAt: minutes(30),
      },
      {
        id: "c3",
        author: "design@teispace.com",
        email: "design@teispace.com",
        verified: true,
        byOwner: false,
        body: "Thanks! Also check the mobile layout.",
        createdAt: minutes(4),
      },
    ],
  },
  {
    id: "c4",
    path: "/",
    anchor: null,
    resolved: false,
    resolvedBy: null,
    resolvedAt: null,
    createdAt: minutes(12),
    comments: [
      {
        id: "c4",
        author: "Teispace QA",
        email: null,
        verified: false,
        byOwner: false,
        body: "Hero image loads slowly on the first visit.",
        createdAt: minutes(12),
      },
    ],
  },
  {
    id: "c5",
    path: "/docs/start",
    anchor: { selector: "#install", x: 0.1, y: 0.2, left: 60, top: 820, vw: 1280, vh: 800 },
    resolved: true,
    resolvedBy: "Teispace",
    resolvedAt: minutes(200),
    createdAt: minutes(300),
    comments: [
      {
        id: "c5",
        author: "Teispace QA",
        email: null,
        verified: false,
        byOwner: false,
        body: "Typo: “instal”.",
        createdAt: minutes(300),
      },
    ],
  },
];

const fronts: FrontView[] = [
  {
    accountId: "acc-personal",
    hostname: "app.teispace.com",
    kind: "offline",
    path: "",
    page: {
      title: "Back soon",
      message: "This site runs on my laptop, which is asleep. Try again in the morning.",
      whenAppDown: false,
    },
    inbox: null,
    script: "tt-offline-1a2b3c4d5e",
    routed: true,
  },
  {
    accountId: "acc-personal",
    hostname: "app.teispace.com",
    kind: "inbox",
    path: "/webhooks/",
    page: null,
    inbox: { maxItems: 500, retentionDays: 7, verify: "stripe" },
    script: "tt-inbox-6f7a8b9c0d",
    routed: true,
  },
];

const inboxItems: InboxItem[] = [
  {
    id: "w3",
    receivedAt: minutes(6),
    method: "POST",
    path: "/webhooks/stripe",
    size: 2_310,
    deliveredAt: null,
    status: null,
    attempts: 0,
    error: null,
  },
  {
    id: "w2",
    receivedAt: minutes(95),
    method: "POST",
    path: "/webhooks/stripe",
    size: 1_904,
    deliveredAt: minutes(20),
    status: 200,
    attempts: 1,
    error: null,
  },
  {
    id: "w1",
    receivedAt: minutes(140),
    method: "POST",
    path: "/webhooks/github",
    size: 8_112,
    deliveredAt: minutes(20),
    status: 204,
    attempts: 1,
    error: null,
  },
];

const frontPlan: PlanView = {
  steps: [
    {
      kind: "frontWorker",
      description: {
        key: "core.front.step.putOffline",
        args: { hostname: "app.teispace.com" },
      },
      command: null,
    },
    {
      kind: "frontWorker",
      description: {
        key: "core.front.step.createRoute",
        args: { pattern: "app.teispace.com/*" },
      },
      command: null,
    },
  ],
  warnings: [{ type: "workerRequests", pattern: "app.teispace.com/*" }],
  requiresConfirmation: false,
  fingerprint: "fp-front",
};

const taps: TapView[] = [];

/** Answers the commands of this feature, or `undefined` to fall through. */
export function commentsMock(cmd: string, payload: Record<string, unknown>): unknown {
  switch (cmd) {
    case "comments_subjects":
      return subjects;
    case "comments_threads":
      return payload["key"] === "snapshot:s1" ? threads : threads.slice(1, 2);
    case "comments_reply":
    case "comments_resolve":
      return threads[0];
    case "comments_forget":
      return null;
    case "inspect_taps":
      return taps;
    case "fronts_list":
      return fronts;
    case "inbox_items":
      return inboxItems;
    case "inbox_deliver":
      return [
        { hostname: "app.teispace.com", path: "/webhooks/", delivered: 1, waiting: 0, error: null },
      ];
    case "fronts_preview":
      return frontPlan;
    case "fronts_undo_change":
      return payload["change"];
    case "fronts_apply":
      return new Promise<Outcome>((resolve) =>
        setTimeout(
          () => resolve({ type: "applied", tunnelId: null, verify: [], connectorError: null }),
          400,
        ),
      );
    default:
      return undefined;
  }
}
