import type { MessageKey } from "@/lib/i18n";

/**
 * How the app helps with each error the core can report (D-066). Every `core.error.*`
 * message is listed, so a new one can't ship without deciding (a test checks it):
 *
 * - `fix`: a card resolves it in place (PermissionFix, ZeroTrustFix, the binary notice).
 * - `link`: the error comes with a button to the Cloudflare page where it's resolved.
 * - `input`: the user corrects what they typed; the message says how.
 * - `retry`: transient or internal; the message and Try Again are the remedy.
 */
export type ErrorHelp =
  | { kind: "fix" }
  | { kind: "link"; label: MessageKey; url: string }
  | { kind: "input" }
  | { kind: "retry" };

const fix = { kind: "fix" } as const;
const input = { kind: "input" } as const;
const retry = { kind: "retry" } as const;

export const errorHelp: Record<string, ErrorHelp> = {
  "core.error.hostname.empty": input,
  "core.error.hostname.invalid": input,
  "core.error.hostname.noDomain": input,
  "core.error.hostname.badWildcard": input,
  "core.error.hostname.tooLong": input,
  "core.error.origin.empty": input,
  "core.error.origin.invalidPort": input,
  "core.error.origin.unsupportedScheme": input,
  "core.error.origin.invalidHost": input,
  "core.error.path.invalid": input,
  "core.error.path.unsupported": input,
  "core.error.network.invalid": input,
  "core.error.network.tooBroad": input,
  "core.error.network.unroutable": input,
  "core.error.routeOrigin.empty": input,
  "core.error.routeOrigin.scheme": input,
  "core.error.routeOrigin.address": input,
  "core.error.routeOrigin.status": input,
  "core.error.accessRule.empty": input,
  "core.error.accessRule.email": input,
  "core.error.accessRule.domain": input,
  "core.error.accessDomain.pathPattern": input,
  "core.error.plan.noZone": {
    kind: "link",
    label: "errorHelp.addDomain",
    url: "https://dash.cloudflare.com/?to=/:account/add-site",
  },
  "core.error.plan.routeExists": input,
  "core.error.plan.noSuchRoute": retry,
  "core.error.plan.noTunnel": retry,
  "core.error.plan.noSuchRecord": retry,
  "core.error.plan.zeroTrustNotSetUp": fix,
  "core.error.plan.noSuchLogin": retry,
  "core.error.plan.accessAppExists": {
    kind: "link",
    label: "errorHelp.openAccessApps",
    url: "https://one.dash.cloudflare.com/?to=/:account/access/apps",
  },
  "core.error.plan.networkRouted": input,
  "core.error.plan.noSuchNetwork": retry,
  "core.error.plan.routedElsewhere": input,
  "core.error.plan.invalidTunnelName": input,
  "core.error.plan.tunnelNameTaken": input,
  "core.error.plan.balancerExists": {
    kind: "link",
    label: "errorHelp.openLoadBalancing",
    url: "https://dash.cloudflare.com/?to=/:account/load-balancing",
  },
  "core.error.plan.notBalanced": retry,
  "core.error.adopt.alreadyHere": retry,
  "core.error.adopt.gone": retry,
  "core.error.adopt.locallyConfigured": input,
  "core.error.engine.stale": fix,
  "core.error.engine.needsConfirmation": fix,
  "core.error.engine.nothingToRestore": retry,
  "core.error.observe.accessPermission": fix,
  "core.error.observe.unknownTunnel": retry,
  "core.error.observe.edgePermission": fix,
  "core.error.observe.serviceTokenPermission": fix,
  "core.error.observe.workersPermission": fix,
  "core.error.cloudflare.permission": fix,
  "core.error.analytics.permission": fix,
  "core.error.analytics.rateLimited": retry,
  "core.error.analytics.notOnPlan": retry,
  "core.error.analytics.noZone": retry,
  "core.error.cloudflare.api": retry,
  "core.error.cloudflare.decode": retry,
  "core.error.cloudflare.network": retry,
  "core.error.cloudflared.invalidVersion": fix,
  "core.error.cloudflared.notFound": fix,
  "core.error.cloudflared.io": retry,
  "core.error.cloudflared.exited": retry,
  "core.error.cloudflared.exitedSignal": retry,
  "core.error.cloudflared.http": retry,
  "core.error.cloudflared.verification": retry,
  "core.error.cloudflared.unsupportedPlatform": retry,
  "core.error.cloudflared.timeout": retry,
  "core.error.store.sqlite": retry,
  "core.error.store.migration": retry,
  "core.error.store.io": retry,
  "core.error.store.decode": retry,
  "core.error.store.closed": retry,
  "core.error.account.invalidToken": input,
  "core.error.account.noAccess": fix,
  "core.error.account.invalidCert": input,
  "core.error.account.notFound": retry,
  "core.error.oauth.timeout": retry,
  "core.error.oauth.denied": retry,
  "core.error.oauth.stateMismatch": retry,
  "core.error.oauth.exchange": retry,
  "core.error.oauth.random": retry,
  "core.error.oauth.noPort": retry,
  "core.error.oauth.noOfflineAccess": retry,
  "core.error.oauth.noCode": retry,
  "core.error.quickShare.noFreePort": retry,
  "core.error.quickShare.notFound": retry,
  "core.error.quickShare.invalidHostHeader": input,
  "core.error.quickShare.config": retry,
  "core.error.inspect.lens": retry,
  "core.error.inspect.unknownTap": retry,
  "core.error.inspect.unknownExchange": retry,
  "core.error.inspect.notWeb": input,
  "core.error.inspect.notRoute": input,
  "core.error.inspect.notInspected": retry,
  "core.error.inspect.tapGone": retry,
  "core.error.inspect.notPaused": retry,
  "core.error.inspect.invalid": input,
  "core.error.shortcut.invalid": input,
  "core.error.shortcut.needsModifier": input,
  "core.error.shortcut.taken": input,
  "core.error.shortcut.unsupported": retry,
  "core.error.connector.alreadyRunning": retry,
  "core.error.connector.notFound": retry,
  "core.error.secret.keychain": retry,
  "core.error.secret.unavailable": retry,
  "core.error.quickShare.folderNeedsInspector": input,
  "core.error.quickShare.notInspected": input,
  "core.error.folder.notFolder": input,
  "core.error.folder.tooBroad": input,
  "core.error.schedule.days": input,
  "core.error.schedule.time": input,
  "core.error.schedule.timeZone": input,
  "core.error.schedule.spec": input,
  // The app, `up` or `serve` must be running, or the terminal share restarted.
  "core.error.pause.ownerGone": retry,
  "core.error.pause.notInspected": input,
  "core.error.pause.noHost": retry,
  "core.error.shareName.placeholder": input,
  "core.error.shareName.noValue": input,
  "core.error.shareName.invalid": input,
};

/** The Cloudflare page that resolves an error, if it has one. */
export function errorLink(key: string | null | undefined) {
  const help = key ? errorHelp[key] : undefined;
  return help?.kind === "link" ? help : null;
}
