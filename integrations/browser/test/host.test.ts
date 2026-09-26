import assert from "node:assert/strict";
import { test } from "node:test";
import { HostClient, HostError } from "../src/host.ts";

/** A browser runtime whose native port answers with `answer`, or fails to start. */
function runtime(answer: (message: { id: number; method: string }) => unknown, missing = false) {
  const runtime: ExtensionRuntime = {
    lastError: undefined,
    connectNative(application) {
      assert.equal(application, "com.teispace.teitunnel");
      const onMessage: ((m: unknown) => void)[] = [];
      const onDisconnect: ((p: ExtensionPort) => void)[] = [];
      const port: ExtensionPort = {
        postMessage(message) {
          queueMicrotask(() => {
            if (missing) {
              runtime.lastError = { message: "Specified native messaging host not found." };
              for (const listener of onDisconnect) listener(port);
              return;
            }
            const reply = answer(message as { id: number; method: string });
            for (const listener of onMessage) listener(reply);
          });
        },
        disconnect() {},
        onMessage: { addListener: (l) => onMessage.push(l) },
        onDisconnect: { addListener: (l) => onDisconnect.push(l) },
      };
      return port;
    },
  };
  return runtime;
}

test("matches answers to requests and turns errors into HostErrors", async () => {
  const client = new HostClient(
    runtime(({ id, method }) =>
      method === "shares.list"
        ? { id, result: [{ id: "qs-1", url: "https://a.trycloudflare.com" }] }
        : { id, error: { code: "appNotRunning", message: "Teitunnel isn't running." } },
    ),
  );
  const [shares, failed] = await Promise.allSettled([
    client.request<{ id: string }[]>("shares.list"),
    client.request("status"),
  ]);
  assert.equal(shares.status === "fulfilled" && shares.value[0]?.id, "qs-1");
  assert.ok(failed.status === "rejected" && failed.reason instanceof HostError);
  assert.equal(failed.status === "rejected" && failed.reason.code, "appNotRunning");
});

test("says how to set it up when the host isn't installed", async () => {
  const client = new HostClient(runtime(() => null, true));
  await assert.rejects(client.request("shares.list"), (error: HostError) => {
    assert.equal(error.code, "hostMissing");
    assert.match(error.message, /Settings ▸ Integrations/);
    return true;
  });
});
