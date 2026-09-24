/**
 * A dependency-free client for Teitunnel's local control connection, for editor
 * extensions and launchers (Node 20+). Bundle it into the extension; it isn't published.
 *
 * ```ts
 * const teitunnel = new ControlClient({ client: { name: "my-tool", version: "1.0.0" }, events: "all" });
 * teitunnel.onEvent((event) => { if (event.type === "sharesChanged") refresh(); });
 * const shares = await teitunnel.listShares();
 * ```
 */

export {
  type ConnectionState,
  ControlClient,
  type ControlClientOptions,
} from "./client.ts";
export {
  currentEnvironment,
  dataDir,
  type Endpoint,
  type Environment,
  IDENTIFIER,
  resolveEndpoint,
} from "./endpoint.ts";
export { ControlError, type ControlErrorKind } from "./errors.ts";
export { frame, LineReader } from "./framing.ts";
export * from "./protocol.ts";
export { openAppCommand, shareLabel, shortUrl } from "./util.ts";
