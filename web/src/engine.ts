// Promise-based client for the engine workers.
//
// One worker answers everything. Scanning a file runs one search per
// column, so `explain` calls are spread over a pool of workers that each
// load the file once, the first time they are needed.

import type {
  DatasetInfo,
  Derived,
  Domain,
  Explained,
  ExplainOptions,
  FitReport,
  Request,
  Response,
  SearchEvent,
  Values,
} from "./types";

/** A call in flight: how to answer it, and who wants its events. */
interface Pending {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  onEvent?: (e: SearchEvent) => void;
}

/** The file loaded last, so pool workers (and new ones) can load it too. */
interface LoadedFile {
  name: string;
  bytes: ArrayBuffer;
  sheet?: string;
  /** A second file whose rows are matched with this one's. */
  with?: { name: string; bytes: ArrayBuffer };
}

/** One worker and its calls in flight. */
class Slot {
  readonly worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  private next = 0;
  private pending = new Map<number, Pending>();
  /** The file this worker holds, or is loading. */
  file?: LoadedFile;
  loading?: Promise<unknown>;

  constructor() {
    this.worker.onmessage = (event: MessageEvent<Response>) => {
      const res = event.data;
      const p = this.pending.get(res.id);
      if (!p) return;
      if ("event" in res) {
        p.onEvent?.(res.event);
        return;
      }
      this.pending.delete(res.id);
      if (res.ok) p.resolve(res.value);
      else p.reject(new Error(res.error));
    };
  }

  /** Calls in flight. */
  get busy(): number {
    return this.pending.size;
  }

  call<T>(req: Request, transfer: Transferable[] = [], onEvent?: (e: SearchEvent) => void): Promise<T> {
    const id = this.next++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, onEvent });
      this.worker.postMessage({ id, req }, transfer);
    });
  }

  load<T>(file: LoadedFile): Promise<T> {
    // The buffer is transferred: send a copy, the original may be needed
    // again (other workers, switching sheets).
    const copy = file.bytes.slice(0);
    const other = file.with ? { name: file.with.name, bytes: file.with.bytes.slice(0) } : undefined;
    this.file = file;
    const loading = this.call<T>(
      { kind: "load", name: file.name, bytes: copy, sheet: file.sheet, with: other },
      other ? [copy, other.bytes] : [copy],
    );
    this.loading = loading;
    return loading;
  }

  /** Waits until this worker holds `file`, loading it if needed. */
  async ensure(file: LoadedFile): Promise<void> {
    if (this.file !== file) this.load(file);
    await this.loading;
  }

  stop() {
    this.worker.terminate();
    for (const p of this.pending.values()) p.reject(new Error("stopped"));
    this.pending.clear();
  }
}

/** Workers used for `explain`: all cores but one, at most 8. */
export function poolSize(): number {
  const cores = typeof navigator !== "undefined" ? navigator.hardwareConcurrency || 2 : 2;
  return Math.max(1, Math.min(8, cores - 1));
}

export class Engine {
  /** `slots[0]` answers everything; the others only `explain`. */
  private slots: Slot[] = [new Slot()];
  private file?: LoadedFile;

  constructor(private readonly size = poolSize()) {}

  /** Number of workers a scan can use. */
  get workers(): number {
    return this.size;
  }

  /**
   * Stops whatever the engine is doing by replacing its workers: a search
   * cannot be interrupted from outside. Pending calls fail, and the file
   * must be loaded again.
   */
  restart() {
    for (const slot of this.slots) slot.stop();
    this.slots = [new Slot()];
    this.file = undefined;
  }

  private get main(): Slot {
    return this.slots[0];
  }

  load(
    name: string,
    bytes: ArrayBuffer,
    sheet?: string,
    other?: { name: string; bytes: ArrayBuffer },
  ): Promise<DatasetInfo> {
    // Keep a copy: the caller's buffer may be detached later.
    const withCopy = other ? { name: other.name, bytes: other.bytes.slice(0) } : undefined;
    this.file = { name, bytes: bytes.slice(0), sheet, with: withCopy };
    // Pool workers load the new file when they are next needed.
    return this.main.load<DatasetInfo>(this.file);
  }

  /**
   * Looks for a formula for `column`, on the least busy worker. Several
   * calls run at the same time on different workers. `onEvent` hears what
   * the search is doing while it runs. With `where`, the call runs on
   * worker `where.slot` when it is given (a worker keeps what a quick look
   * at a column found, so checking it there skips searching again), and
   * `where.slot` is set to the worker used.
   */
  async explain(
    column: string,
    options: ExplainOptions,
    onEvent?: (e: SearchEvent) => void,
    where?: { slot?: number },
  ): Promise<Explained> {
    const file = this.file;
    if (!file) throw new Error("no file loaded");
    const pinned = where?.slot !== undefined ? this.slots[where.slot] : undefined;
    if (!pinned && this.slots.length < this.size && this.slots.every((s) => s.busy > 0)) this.slots.push(new Slot());
    const slot = pinned ?? this.slots.reduce((a, b) => (b.busy < a.busy ? b : a));
    if (where) where.slot = this.slots.indexOf(slot);
    await slot.ensure(file);
    return slot.call({ kind: "explain", column, options }, [], onEvent);
  }

  /** Checks a formula written by the user; names that are not columns are fitted constants. */
  formula(column: string, text: string): Promise<Derived> {
    return this.main.call({ kind: "formula", column, text });
  }

  /**
   * Diagnoses the exceptions of `derived` together, so a wrong cell that
   * breaks several formulas is blamed once. Returns the same list with the
   * exceptions diagnosed again.
   */
  review(ignore: string[], derived: Derived[]): Promise<Derived[]> {
    return this.main.call({ kind: "review", options: { ignore, derived } });
  }

  /** The given rows (numbered from 1) as text, in the same order. */
  rows(rows: number[]): Promise<string[][]> {
    return this.main.call({ kind: "rows", rows });
  }

  /** What `column` holds and what `formula` gives, for every row. */
  values(column: string, formula: string): Promise<Values> {
    return this.main.call({ kind: "values", column, formula });
  }

  /** The domains of the built-in formula library. */
  domains(): Promise<Domain[]> {
    return this.main.call({ kind: "domains" });
  }

  fit(target: string, inputs: string[]): Promise<FitReport> {
    return this.main.call({ kind: "fit", target, options: { inputs } });
  }
}
