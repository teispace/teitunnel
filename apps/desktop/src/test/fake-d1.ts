// A D1 binding for Worker tests, on Node's built-in SQLite (the same engine D1 runs):
// prepare(sql).bind(...).all()/first()/run() and batch() as a transaction.
import { DatabaseSync } from "node:sqlite";

type Value = string | number | bigint | null | Uint8Array | ArrayBuffer;

function sqlValue(value: Value) {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  return value;
}

export class FakeStatement {
  constructor(
    private readonly db: DatabaseSync,
    readonly sql: string,
    private readonly params: Value[] = [],
  ) {}

  bind(...params: Value[]) {
    return new FakeStatement(this.db, this.sql, params);
  }

  private statement() {
    // D1 accepts ?1, ?2… (numbered) like SQLite; node:sqlite binds them positionally.
    return this.db.prepare(this.sql);
  }

  async all() {
    const results = this.statement().all(...this.params.map(sqlValue));
    return { results, success: true, meta: {} };
  }

  async first<T = Record<string, unknown>>(): Promise<T | null> {
    const row = this.statement().get(...this.params.map(sqlValue));
    return (row as T) ?? null;
  }

  async run() {
    const info = this.statement().run(...this.params.map(sqlValue));
    return {
      success: true,
      meta: { changes: Number(info.changes), last_row_id: Number(info.lastInsertRowid) },
    };
  }

  runSync() {
    return this.statement().run(...this.params.map(sqlValue));
  }
}

export class FakeD1 {
  readonly db = new DatabaseSync(":memory:");

  prepare(sql: string) {
    return new FakeStatement(this.db, sql);
  }

  async batch(statements: FakeStatement[]) {
    this.db.exec("BEGIN");
    try {
      const out = statements.map((s) => s.runSync());
      this.db.exec("COMMIT");
      return out.map(() => ({ success: true }));
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
  }

  rows(sql: string): Record<string, unknown>[] {
    return this.db.prepare(sql).all() as Record<string, unknown>[];
  }
}
