import { MAX_MESSAGE } from "./protocol.ts";

/**
 * Splits a byte stream into lines (one JSON message each), bounded: a line longer than
 * `max` bytes is never buffered whole; `push` throws and the stream is unusable.
 */
export class LineReader {
  readonly #max: number;
  #chunks: Buffer[] = [];
  #size = 0;

  constructor(max: number = MAX_MESSAGE) {
    this.#max = max;
  }

  /** Adds bytes; returns the complete, non-empty lines (without `\r\n`). */
  push(data: Buffer): string[] {
    const lines: string[] = [];
    let start = 0;
    for (;;) {
      const end = data.indexOf(0x0a, start);
      const piece = data.subarray(start, end === -1 ? data.length : end);
      if (this.#size + piece.length > this.#max) {
        throw new RangeError("message too large");
      }
      if (end === -1) {
        if (piece.length > 0) {
          this.#chunks.push(piece);
          this.#size += piece.length;
        }
        return lines;
      }
      const line = Buffer.concat([...this.#chunks, piece])
        .toString("utf8")
        .replace(/\r$/, "");
      this.#chunks = [];
      this.#size = 0;
      if (line.trim() !== "") lines.push(line);
      start = end + 1;
    }
  }
}

/** One message as a line. */
export function frame(message: unknown): string {
  return `${JSON.stringify(message)}\n`;
}
