import { describe, expect, it } from "vitest";
import { call, IpcError, toIpcError } from "./client";

describe("toIpcError", () => {
  it("keeps the fields of an AppError", () => {
    const error = toIpcError({ code: "internal", message: "Nope.", hint: "Retry.", field: null });
    expect(error).toBeInstanceOf(IpcError);
    expect(error.message).toBe("Nope.");
    expect(error.hint).toBe("Retry.");
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
      call(Promise.reject({ code: "internal", message: "x", hint: null, field: null })),
    ).rejects.toBeInstanceOf(IpcError);
  });
});
