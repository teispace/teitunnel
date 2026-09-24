import { ErrorCode } from "./protocol.ts";

/**
 * What went wrong, for code to branch on:
 * - `notInstalled`: no token file (Teitunnel never ran on this computer as this user).
 * - `notRunning`: the app isn't running (offer to open it with `OPEN_APP_URL`).
 * - `disabled`: the person turned the control connection off in Settings.
 * - `unauthorized`: the token was refused (the app was reinstalled; retrying reads it again).
 * - `unsupportedProtocol`: the app is too old or too new for this client.
 * - `declined`: the person said no to a change (handle quietly; don't show an error).
 * - `timeout`, `disconnected`, `tooLarge`, `rpc` (any other error from the app).
 */
export type ControlErrorKind =
  | "notInstalled"
  | "notRunning"
  | "disabled"
  | "unauthorized"
  | "unsupportedProtocol"
  | "declined"
  | "timeout"
  | "disconnected"
  | "tooLarge"
  | "rpc";

const MESSAGES: Record<ControlErrorKind, string> = {
  notInstalled:
    "Teitunnel isn't set up on this computer. Install it from teitunnel.teispace.com and open it once.",
  notRunning: "Teitunnel isn't running. Open Teitunnel and try again.",
  disabled:
    "Teitunnel's connection for extensions is turned off. Turn on Settings ▸ Integrations ▸ Allow connections.",
  unauthorized: "Teitunnel didn't accept this extension's token. Try again.",
  unsupportedProtocol:
    "This extension and your Teitunnel app don't speak the same version. Update both.",
  declined: "The change wasn't allowed in Teitunnel.",
  timeout: "Teitunnel didn't answer in time.",
  disconnected: "The connection to Teitunnel closed.",
  tooLarge: "A message was too large.",
  rpc: "Teitunnel couldn't do that.",
};

/** An error from the control connection. Its message never contains the token. */
export class ControlError extends Error {
  readonly kind: ControlErrorKind;
  /** The JSON-RPC error code, for errors the app sent. */
  readonly code: number | undefined;
  /** Details the app sent (e.g. the new plan for `stale`). */
  readonly data: unknown;

  constructor(kind: ControlErrorKind, message?: string, code?: number, data?: unknown) {
    super(message ?? MESSAGES[kind]);
    this.name = "ControlError";
    this.kind = kind;
    this.code = code;
    this.data = data;
  }

  /** The person said no: nothing to report. */
  get declined(): boolean {
    return this.kind === "declined";
  }

  /** Opening the app (`OPEN_APP_URL`) would help. */
  get appUnavailable(): boolean {
    return this.kind === "notRunning" || this.kind === "notInstalled";
  }

  /** An error answer from the app. */
  static fromRpc(error: { code: number; message: string; data?: unknown }): ControlError {
    const kind: ControlErrorKind =
      error.code === ErrorCode.declined
        ? "declined"
        : error.code === ErrorCode.unauthorized
          ? "unauthorized"
          : error.code === ErrorCode.disabled
            ? "disabled"
            : error.code === ErrorCode.unsupportedProtocol
              ? "unsupportedProtocol"
              : error.code === ErrorCode.timeout
                ? "timeout"
                : error.code === ErrorCode.tooLarge
                  ? "tooLarge"
                  : "rpc";
    const message = kind === "rpc" ? error.message : MESSAGES[kind];
    return new ControlError(kind, message, error.code, error.data);
  }
}

/** Whether a socket error means nothing is listening (the app isn't running). */
export function isNotListening(error: unknown): boolean {
  const code = (error as { code?: unknown } | null)?.code;
  return code === "ENOENT" || code === "ECONNREFUSED" || code === "EPIPE" || code === "ENOTSOCK";
}
