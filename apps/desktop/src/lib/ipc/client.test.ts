import { describe, expect, it } from "vitest";
import { call, IpcError, toIpcError } from "./client";

describe("toIpcError", () => {
  it("keeps the fields of an AppError", () => {
    const error = toIpcError({
      code: "invalidInput",
      message: { key: "core.error.plan.noZone", args: { hostname: "a.xyz.com" } },
      hint: { key: "core.app.internalHint", args: {} },
      field: "hostname",
    });
    expect(error).toBeInstanceOf(IpcError);
    // Translated from the catalog, with its arguments filled in.
    expect(error.message).toBe(
      "a.xyz.com isn't in any of this account's domains. Add the domain to Cloudflare first.",
    );
    expect(error.hint).toMatch(/^Try again\./);
    expect(error.key).toBe("core.error.plan.noZone");
  });

  it("wraps unknown values", () => {
    expect(toIpcError("boom").message).toBe("boom");
    expect(toIpcError(new Error("bad")).message).toBe("bad");
    expect(toIpcError(null).code).toBe("internal");
  });
});

describe("call", () => {
  it("passes results through and converts rejections", async () => {
    await expect(call(Promise.resolve(3))).resolves.toBe(3);
    await expect(
      call(
        Promise.reject({
          code: "internal",
          message: { key: "core.raw", args: { text: "x" } },
          hint: null,
          field: null,
        }),
      ),
    ).rejects.toBeInstanceOf(IpcError);
  });
});
