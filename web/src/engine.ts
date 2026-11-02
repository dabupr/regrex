// Promise-based client for the engine worker.

import type { DatasetInfo, Derived, FitReport, Request, Response } from "./types";

export class Engine {
  private worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  private next = 0;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();

  constructor() {
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

  explain(column: string, ignore: string[]): Promise<Derived | null> {
    return this.call({ kind: "explain", column, options: { ignore } });
  }

  fit(target: string, inputs: string[]): Promise<FitReport> {
    return this.call({ kind: "fit", target, options: { inputs } });
  }
}
