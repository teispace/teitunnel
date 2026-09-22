import type { AppError, ErrorCode } from "./bindings";

/** An error returned by a Rust command, with the fields of `AppError`. */
export class IpcError extends Error {
  readonly code: ErrorCode;
  readonly hint: string | null;
  readonly field: string | null;

  constructor(error: AppError) {
    super(error.message);
    this.name = "IpcError";
    this.code = error.code;
    this.hint = error.hint;
    this.field = error.field;
  }
}

function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value &&
    typeof value.message === "string"
  );
}

/** Normalises anything thrown by `invoke` into an `IpcError`. */
export function toIpcError(error: unknown): IpcError {
  if (error instanceof IpcError) return error;
  if (isAppError(error)) return new IpcError(error);
  const message = error instanceof Error ? error.message : String(error);
  return new IpcError({ code: "internal", message, hint: null, field: null });
}

/**
 * Awaits a generated command and rethrows failures as `IpcError`, so TanStack Query
 * sees typed errors. Use it in `queries.ts`; components never call commands directly.
 */
export async function call<T>(command: Promise<T>): Promise<T> {
  try {
    return await command;
  } catch (error) {
    throw toIpcError(error);
  }
}
