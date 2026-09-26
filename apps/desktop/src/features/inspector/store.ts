import type { ExchangeRow, LiveBatch } from "@/lib/ipc/bindings";

/** Requests kept for the list at most; the oldest go first (10,000). */
export const MAX_ROWS = 10_000;

/**
 * The list's requests, newest first, updated from live batches. Kept outside React so a
 * burst of batches costs one array copy each, not a re-render per request; `rows` is a
 * new array only when something changed (for `useSyncExternalStore`).
 */
export class ExchangeStore {
  private byId = new Map<string, ExchangeRow>();
  private order: string[] = [];
  private snapshot: readonly ExchangeRow[] = [];
  private listeners = new Set<() => void>();
  /** Only this tap's requests (`null`: every tap). */
  private readonly tap: string | null;
  /** Requests kept at most. */
  private readonly capacity: number;

  constructor(tap: string | null = null, capacity = MAX_ROWS) {
    this.tap = tap;
    this.capacity = capacity;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  rows = (): readonly ExchangeRow[] => this.snapshot;

  /** Replaces everything with a page read from the inspector (newest first). */
  reset(rows: readonly ExchangeRow[]) {
    this.byId = new Map(rows.map((row) => [row.id, row]));
    this.order = rows.map((row) => row.id);
    this.trim();
    this.emit();
  }

  /** Adds an older page at the end (Load earlier). */
  appendOlder(rows: readonly ExchangeRow[]) {
    for (const row of rows) {
      if (this.byId.has(row.id)) continue;
      this.byId.set(row.id, row);
      this.order.push(row.id);
    }
    this.trim();
    this.emit();
  }

  /**
   * Applies a live batch; returns the number of new requests. `accept` decides whether a
   * new request belongs in the list (a search the batch can't be matched against).
   */
  apply(batch: LiveBatch, accept: (row: ExchangeRow) => boolean = () => true): number {
    let changed = false;
    for (const tap of batch.cleared) {
      if (tap === null || tap === this.tap || this.tap === null) {
        this.clear(tap);
        changed = true;
      }
    }
    const added: string[] = [];
    for (const row of batch.exchanges) {
      if (this.tap !== null && row.tap !== this.tap) continue;
      if (this.byId.has(row.id)) {
        this.byId.set(row.id, row);
        changed = true;
      } else if (accept(row)) {
        this.byId.set(row.id, row);
        added.push(row.id);
      }
    }
    if (added.length > 0) {
      // Batches list the oldest change first; the list is newest first.
      this.order = [...added.reverse(), ...this.order];
      this.trim();
      changed = true;
    }
    if (changed) this.emit();
    return added.length;
  }

  private clear(tap: string | null) {
    if (tap === null) {
      this.byId.clear();
      this.order = [];
      return;
    }
    this.order = this.order.filter((id) => this.byId.get(id)?.tap !== tap);
    for (const [id, row] of this.byId) if (row.tap === tap) this.byId.delete(id);
  }

  private trim() {
    if (this.order.length <= this.capacity) return;
    for (const id of this.order.splice(this.capacity)) this.byId.delete(id);
  }

  private emit() {
    this.snapshot = this.order.flatMap((id) => {
      const row = this.byId.get(id);
      return row ? [row] : [];
    });
    for (const listener of this.listeners) listener();
  }
}
