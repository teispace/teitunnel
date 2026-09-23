import { translate } from "@/lib/i18n";
import type { AppError, ErrorCode, Text } from "./bindings";

/**
 * An error returned by a Rust command, with the fields of `AppError`. Its message and
 * hint arrive as catalog keys and are translated here, once (D-062).
 */
export class IpcError extends Error {
  readonly code: ErrorCode;
  readonly hint: string | null;
  readonly field: string | null;
  /** The message's catalog key, to recognise a specific error without its wording. */
  readonly key: string | null;

  constructor(error: AppError) {
    super(translate(error.message));
    this.name = "IpcError";
    this.code = error.code;
    this.hint = error.hint ? translate(error.hint) : null;
    this.field = error.field;
    this.key = error.message.key;
  }
}

const isText = (value: unknown): value is Text =>
  typeof value === "object" && value !== null && "key" in value && "args" in value;

function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value &&
    isText(value.message)
  );
}

/** Normalises anything thrown by `invoke` into an `IpcError`. */
export function toIpcError(error: unknown): IpcError {
  if (error instanceof IpcError) return error;
  if (isAppError(error)) return new IpcError(error);
  const text = error instanceof Error ? error.message : String(error);
  return new IpcError({
    code: "internal",
    message: { key: "core.raw", args: { text } },
    hint: null,
    field: null,
  });
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
