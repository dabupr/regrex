// Promise-based client for the engine worker.

import type { DatasetInfo, Derived, ExplainOptions, FitReport, Request, Response } from "./types";

export class Engine {
  private worker = Engine.spawn();
  private next = 0;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();

  private static spawn(): Worker {
    return new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  }

  constructor() {
    this.listen();
  }

  /**
   * Stops whatever the engine is doing by replacing its worker: a search
   * cannot be interrupted from outside. Pending calls fail, and the file
   * must be loaded again.
   */
  restart() {
    this.worker.terminate();
    for (const p of this.pending.values()) p.reject(new Error("stopped"));
    this.pending.clear();
    this.worker = Engine.spawn();
    this.listen();
  }

  private listen() {
    this.worker.onmessage = (event: MessageEvent<Response>) => {
      const res = event.data;
      const p = this.pending.get(res.id);
      if (!p) return;
      this.pending.delete(res.id);
      if (res.ok) p.resolve(res.value);
      else p.reject(new Error(res.error));
    };
  }

  private call<T>(req: Request, transfer: Transferable[] = []): Promise<T> {
    const id = this.next++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage({ id, req }, transfer);
    });
  }

  load(name: string, bytes: ArrayBuffer, sheet?: string): Promise<DatasetInfo> {
    // Keep a copy: the buffer is transferred and may be needed to switch sheets.
    const copy = bytes.slice(0);
    return this.call({ kind: "load", name, bytes: copy, sheet }, [copy]);
  }

  explain(column: string, options: ExplainOptions): Promise<Derived | null> {
    return this.call({ kind: "explain", column, options });
  }

  /** Checks a formula written by the user; names that are not columns are fitted constants. */
  formula(column: string, text: string): Promise<Derived> {
    return this.call({ kind: "formula", column, text });
  }

  /** Every formula found so far, diagnosed together so a wrong cell is blamed once. */
  review(ignore: string[]): Promise<Derived[]> {
    return this.call({ kind: "review", options: { ignore } });
  }

  /** The given rows (numbered from 1) as text, in the same order. */
  rows(rows: number[]): Promise<string[][]> {
    return this.call({ kind: "rows", rows });
  }

  fit(target: string, inputs: string[]): Promise<FitReport> {
    return this.call({ kind: "fit", target, options: { inputs } });
  }
}
