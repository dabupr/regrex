// Parquet, Arrow and SQLite files in the browser. Each reader is a separate
// chunk loaded only when such a file is opened, so the page stays small.
// The table is handed to the engine as JSON (an array of rows, the first
// holding the column names), which it already reads.

export type Columnar = "parquet" | "arrow" | "sqlite";

/** The format of a file, from its first bytes or else its extension. */
export function columnarFormat(name: string, bytes: ArrayBuffer): Columnar | null {
  const head = new TextDecoder("latin1").decode(new Uint8Array(bytes, 0, Math.min(16, bytes.byteLength)));
  if (head.startsWith("PAR1")) return "parquet";
  if (head.startsWith("ARROW1") || head.startsWith("FEA1")) return "arrow";
  if (head.startsWith("SQLite format 3")) return "sqlite";
  const ext = name.toLowerCase().split(".").pop() ?? "";
  if (ext === "parquet" || ext === "pq") return "parquet";
  if (["arrow", "feather", "arrows", "ipc"].includes(ext)) return "arrow";
  if (["sqlite", "sqlite3", "db"].includes(ext)) return "sqlite";
  return null;
}

/** A table read from a columnar file: header first, then the rows. */
export interface Converted {
  /** The table as JSON bytes for the engine. */
  json: ArrayBuffer;
  /** Tables of a SQLite database, offered like the sheets of a spreadsheet. */
  sheets: string[];
}

/** A cell as JSON: numbers stay numbers, big integers and dates become text. */
function cell(v: unknown): string | number | null {
  if (v === null || v === undefined) return null;
  if (typeof v === "number") return Number.isFinite(v) ? v : null;
  if (typeof v === "bigint") return v.toString();
  if (typeof v === "boolean") return v ? 1 : 0;
  if (v instanceof Date) return v.toISOString();
  if (typeof v === "string") return v;
  if (v instanceof Uint8Array) return null;
  return String(v);
}

function encode(header: string[], rows: unknown[][]): ArrayBuffer {
  const table = [header, ...rows.map((r) => r.map(cell))];
  return new TextEncoder().encode(JSON.stringify(table)).buffer as ArrayBuffer;
}

/** Reads a Parquet, Arrow or SQLite file; `sheet` names the SQLite table. */
export async function readColumnar(format: Columnar, bytes: ArrayBuffer, sheet?: string): Promise<Converted> {
  switch (format) {
    case "parquet":
      return readParquet(bytes);
    case "arrow":
      return readArrow(bytes);
    case "sqlite":
      return readSqlite(bytes, sheet);
  }
}

async function readParquet(bytes: ArrayBuffer): Promise<Converted> {
  const [{ parquetMetadata, parquetSchema, parquetRead }, { compressors }] = await Promise.all([
    import("hyparquet"),
    import("hyparquet-compressors"),
  ]);
  const metadata = parquetMetadata(bytes);
  const header = parquetSchema(metadata).children.map((c) => c.element.name);
  const rows = await new Promise<unknown[][]>((resolve, reject) => {
    parquetRead({ file: bytes, metadata, compressors, onComplete: (data) => resolve(data as unknown[][]) }).catch(reject);
  });
  return { json: encode(header, rows), sheets: [] };
}

async function readArrow(bytes: ArrayBuffer): Promise<Converted> {
  if (new TextDecoder("latin1").decode(new Uint8Array(bytes, 0, 4)) === "FEA1") {
    throw new Error("this is a Feather version 1 file; save it again as Feather version 2 (Arrow IPC)");
  }
  const { tableFromIPC } = await import("apache-arrow");
  const table = tableFromIPC(new Uint8Array(bytes));
  const header = table.schema.fields.map((f) => f.name);
  const columns = header.map((_, i) => table.getChildAt(i));
  const rows: unknown[][] = [];
  for (let r = 0; r < table.numRows; r++) rows.push(columns.map((c) => c?.get(r)));
  return { json: encode(header, rows), sheets: [] };
}

async function readSqlite(bytes: ArrayBuffer, sheet?: string): Promise<Converted> {
  const [{ default: initSqlJs }, { default: wasmUrl }] = await Promise.all([
    import("sql.js"),
    import("sql.js/dist/sql-wasm.wasm?url"),
  ]);
  const SQL = await initSqlJs({ locateFile: () => wasmUrl });
  const db = new SQL.Database(new Uint8Array(bytes));
  try {
    const list = db.exec(
      "SELECT name FROM sqlite_schema WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY type = 'view', rowid",
    );
    const tables = (list[0]?.values ?? []).map((v) => String(v[0]));
    if (tables.length === 0) throw new Error("the database has no tables");
    const name = sheet && tables.includes(sheet) ? sheet : tables[0];
    const result = db.exec(`SELECT * FROM "${name.replace(/"/g, '""')}"`)[0];
    const header = result?.columns ?? db.exec(`PRAGMA table_info("${name.replace(/"/g, '""')}")`)[0]?.values.map((v) => String(v[1])) ?? [];
    return { json: encode(header, result?.values ?? []), sheets: tables };
  } finally {
    db.close();
  }
}
