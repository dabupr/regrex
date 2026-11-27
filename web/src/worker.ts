// Runs the Rust engine off the main thread so the page stays responsive.

import init, { Dataset, library } from "./wasm/regrex_wasm.js";
import type { Request, Response } from "./types";

const ready = init();
let dataset: Dataset | undefined;

function handle(req: Request, id: number): unknown {
  switch (req.kind) {
    case "load":
      dataset?.free();
      dataset = new Dataset(
        req.name,
        new Uint8Array(req.bytes),
        req.sheet,
        req.with?.name,
        req.with ? new Uint8Array(req.with.bytes) : undefined,
      );
      return JSON.parse(dataset.info());
    case "explain":
      return JSON.parse(
        need().explain_detailed(req.column, JSON.stringify(req.options), (event: string) => {
          const res: Response = { id, event: JSON.parse(event) };
          self.postMessage(res);
        }),
      );
    case "fit":
      return JSON.parse(need().fit(req.target, JSON.stringify(req.options)));
    case "formula":
      return JSON.parse(need().formula(req.column, req.text));
    case "review":
      return JSON.parse(need().review(JSON.stringify(req.options)));
    case "rows":
      return JSON.parse(need().rows(JSON.stringify(req.rows)));
    case "values":
      return JSON.parse(need().values(req.column, req.formula));
    case "domains":
      return (JSON.parse(library()) as { domains: unknown[] }).domains;
  }
}

function need(): Dataset {
  if (!dataset) throw new Error("no file loaded");
  return dataset;
}

self.onmessage = async (event: MessageEvent<{ id: number; req: Request }>) => {
  const { id, req } = event.data;
  let res: Response;
  try {
    await ready;
    res = { id, ok: true, value: handle(req, id) };
  } catch (e) {
    res = { id, ok: false, error: e instanceof Error ? e.message : String(e) };
  }
  self.postMessage(res);
};
