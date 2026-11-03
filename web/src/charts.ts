// Small SVG charts for the results: what a column holds against what its
// formula gives, and how the columns depend on each other. Colours come from
// CSS classes, so both follow the light and dark themes.

import type { Derived, Values } from "./types";

const SVG = "http://www.w3.org/2000/svg";

function svg<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | number> = {},
  ...children: (Node | string)[]
): SVGElementTagNameMap[K] {
  const node = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  node.append(...children);
  return node;
}

/** A tooltip shown on hover (and read by screen readers). */
const title = (text: string) => svg("title", {}, text);

/** Short number for axes and tooltips. */
export function short(x: number): string {
  if (x === 0) return "0";
  const a = Math.abs(x);
  if (a >= 1e6 || a < 1e-3) return x.toExponential(2).replace(/\.?0+e/, "e");
  return String(Number(x.toPrecision(a >= 1000 ? 6 : 4)));
}

/** About `count` round tick values covering [lo, hi]. */
function ticks(lo: number, hi: number, count: number): number[] {
  if (!(hi > lo)) return [lo];
  const raw = (hi - lo) / count;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let t = Math.ceil(lo / step) * step; t <= hi + step * 1e-9; t += step) out.push(Number(t.toPrecision(12)));
  return out;
}

/** At most this many dots are drawn; rows that break the formula are always kept. */
const MAX_DOTS = 2000;

/**
 * Scatter plot of what the file holds (y) against what the formula gives (x),
 * one dot per row. Rows on the diagonal follow the formula. Rows that break it
 * are drawn in the warning colour, rows that follow a second formula in green.
 */
export function scatter(d: Derived, values: Values): SVGSVGElement {
  const broken = new Set(d.exception_rows);
  const other = new Set(d.other?.rows ?? []);
  const points: { row: number; x: number; y: number; kind: "ok" | "bad" | "other" }[] = [];
  values.actual.forEach((y, i) => {
    const x = values.predicted[i];
    if (y === null || x === null) return;
    const row = i + 1;
    const kind = other.has(row) ? "other" : broken.has(row) ? "bad" : "ok";
    points.push({ row, x, y, kind });
  });
  const okCount = points.filter((p) => p.kind === "ok").length;
  const stride = Math.max(1, Math.ceil(okCount / MAX_DOTS));
  let seen = 0;
  const shown = points.filter((p) => p.kind !== "ok" || seen++ % stride === 0);

  const W = 360, H = 260, L = 52, R = 12, T = 12, B = 38;
  const all = shown.flatMap((p) => [p.x, p.y]);
  let lo = Math.min(...all), hi = Math.max(...all);
  if (!(hi > lo)) { lo -= 1; hi += 1; }
  const pad = (hi - lo) * 0.04;
  lo -= pad; hi += pad;
  const sx = (v: number) => L + ((v - lo) / (hi - lo)) * (W - L - R);
  const sy = (v: number) => H - B - ((v - lo) / (hi - lo)) * (H - T - B);

  const chart = svg("svg", {
    viewBox: `0 0 ${W} ${H}`,
    class: "chart scatter",
    role: "img",
    "aria-label": `${d.column} in the file against ${d.column} = ${d.formula}, one dot per row`,
  });
  for (const t of ticks(lo, hi, 4)) {
    chart.append(
      svg("line", { x1: L, x2: W - R, y1: sy(t), y2: sy(t), class: "grid" }),
      svg("text", { x: L - 6, y: sy(t) + 4, class: "tick", "text-anchor": "end" }, short(t)),
      svg("text", { x: sx(t), y: H - B + 16, class: "tick", "text-anchor": "middle" }, short(t)),
    );
  }
  chart.append(
    svg("line", { x1: sx(lo), y1: sy(lo), x2: sx(hi), y2: sy(hi), class: "diagonal" }, title("Rows on this line follow the formula")),
    svg("text", { x: (L + W - R) / 2, y: H - 4, class: "axis-label", "text-anchor": "middle" }, "formula gives"),
    svg("text", { x: 12, y: (T + H - B) / 2, class: "axis-label", "text-anchor": "middle", transform: `rotate(-90 12 ${(T + H - B) / 2})` }, "in the file"),
  );
  // Rows that break the formula go on top.
  const order = { ok: 0, other: 1, bad: 2 };
  for (const p of shown.sort((a, b) => order[a.kind] - order[b.kind])) {
    const what = p.kind === "bad" ? "breaks the formula" : p.kind === "other" ? "follows the second formula" : "follows the formula";
    chart.append(
      svg(
        "circle",
        { cx: sx(p.x), cy: sy(p.y), r: p.kind === "ok" ? 3 : 4.5, class: `dot ${p.kind}`, "data-row": p.row },
        title(`Row ${p.row} ${what}: ${short(p.y)} in the file, formula gives ${short(p.x)}`),
      ),
    );
  }
  return chart;
}

/** The legend for {@link scatter}, as HTML so it wraps on small screens. */
export function scatterLegend(d: Derived): HTMLElement {
  const item = (kind: string, text: string) => {
    const li = document.createElement("li");
    const dot = document.createElement("span");
    dot.className = `swatch ${kind}`;
    li.append(dot, text);
    return li;
  };
  const list = document.createElement("ul");
  list.className = "legend";
  list.append(item("ok", "follows the formula"));
  if (d.other) list.append(item("other", "follows the second formula"));
  if (d.exception_rows.length > (d.other?.rows.length ?? 0)) list.append(item("bad", "breaks it"));
  return list;
}

/**
 * Columns as boxes and arrows from each input to each column computed from
 * it, left to right. Columns that depend on each other (net = total - vat and
 * total = net + vat) are placed once; the arrow that goes back is drawn as an
 * arc. Hovering a computed column shows its formula.
 */
export function dependencyGraph(derived: Derived[]): SVGSVGElement | null {
  if (derived.length === 0) return null;
  const byColumn = new Map(derived.map((d) => [d.column, d]));
  const names: string[] = [];
  for (const d of derived) for (const n of [...d.inputs, d.column]) if (!names.includes(n)) names.push(n);

  // Two columns computed from each other (net = total - vat, total = net + vat)
  // get one arrow with two heads, and do not push each other to the right.
  const uses = (a: string, b: string) => byColumn.get(a)?.inputs.includes(b) ?? false;
  const mutual = (a: string, b: string) => uses(a, b) && uses(b, a);
  // Longest path from the plain columns, ignoring arrows that close a loop.
  const level = new Map<string, number>();
  const visiting = new Set<string>();
  const levelOf = (n: string): number => {
    const known = level.get(n);
    if (known !== undefined) return known;
    const d = byColumn.get(n);
    if (!d) return level.set(n, 0), 0;
    visiting.add(n);
    let l = 0;
    for (const i of d.inputs) if (!visiting.has(i) && !mutual(n, i)) l = Math.max(l, levelOf(i) + 1);
    visiting.delete(n);
    level.set(n, l);
    return l;
  };
  names.forEach(levelOf);
  const columns: string[][] = [];
  for (const n of names) (columns[level.get(n)!] ??= []).push(n);
  // Fewer crossings: order each column by the mean position of the columns
  // it is connected to, sweeping right then left a few times.
  const neighbours = new Map<string, string[]>(names.map((n) => [n, []]));
  for (const d of derived) {
    for (const i of d.inputs) {
      neighbours.get(d.column)!.push(i);
      neighbours.get(i)!.push(d.column);
    }
  }
  const rank = new Map<string, number>();
  const reindex = () => columns.forEach((col) => col.forEach((n, i) => rank.set(n, i - (col.length - 1) / 2)));
  reindex();
  for (let sweep = 0; sweep < 4; sweep++) {
    const order = sweep % 2 === 0 ? columns : [...columns].reverse();
    for (const col of order) {
      const centre = (n: string) => {
        const ns = neighbours.get(n)!;
        return ns.length ? ns.reduce((a, m) => a + rank.get(m)!, 0) / ns.length : rank.get(n)!;
      };
      col.sort((a, b) => centre(a) - centre(b));
      reindex();
    }
  }

  const BW = 120, BH = 30, GX = 64, GY = 34, P = 16;
  const tallest = Math.max(...columns.map((c) => c.length));
  const W = P * 2 + columns.length * BW + (columns.length - 1) * GX;
  const boxesBottom = P + tallest * BH + (tallest - 1) * GY;
  const back = derived.flatMap((d) =>
    d.inputs.filter((i) => level.get(i)! >= level.get(d.column)! && !mutual(d.column, i)).map((i) => [i, d.column]),
  );
  // Arcs above the boxes (edges that skip a column) and below (edges that go back).
  const TOP = derived.some((d) => d.inputs.some((i) => level.get(d.column)! - level.get(i)! > 1)) ? 28 : 0;
  const H = boxesBottom + P + TOP + (back.length ? 18 + back.length * 12 : 0);
  const pos = new Map<string, { x: number; y: number }>();
  columns.forEach((col, ci) => {
    const offset = ((tallest - col.length) * (BH + GY)) / 2;
    col.forEach((n, ri) => pos.set(n, { x: P + ci * (BW + GX), y: P + TOP + offset + ri * (BH + GY) }));
  });

  const graph = svg("svg", {
    viewBox: `0 0 ${W} ${H}`,
    width: W,
    class: "chart graph",
    role: "img",
    "aria-label": `How the columns depend on each other: ${derived.map((d) => `${d.column} = ${d.formula}`).join("; ")}`,
  });
  graph.append(
    svg(
      "defs",
      {},
      svg("marker", { id: "arrow", viewBox: "0 0 10 10", refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: "auto-start-reverse" },
        svg("path", { d: "M 0 0 L 10 5 L 0 10 z", class: "arrow-head" })),
    ),
  );
  let backIndex = 0;
  const top = P + TOP;
  for (const d of derived) {
    const to = pos.get(d.column)!;
    for (const input of d.inputs) {
      const from = pos.get(input)!;
      const span = level.get(d.column)! - level.get(input)!;
      const both = mutual(d.column, input);
      // A two-headed arrow is drawn once, from the left (or upper) column.
      if (both && (span < 0 || (span === 0 && from.y > to.y))) continue;
      let path: string;
      if (both && span === 0) {
        // Same column: straight down when next to each other, else an arc on the right.
        const x = from.x + BW / 2;
        if (Math.abs(to.y - from.y) <= BH + GY + 1) {
          path = `M ${x} ${from.y + BH + 2} L ${x} ${to.y - 2}`;
        } else {
          const right = from.x + BW + 18;
          path = `M ${from.x + BW + 2} ${from.y + BH / 2} C ${right} ${from.y + BH / 2}, ${right} ${to.y + BH / 2}, ${to.x + BW + 2} ${to.y + BH / 2}`;
        }
      } else if (span === 1) {
        const x1 = from.x + BW + (both ? 2 : 0), y1 = from.y + BH / 2, x2 = to.x - 2, y2 = to.y + BH / 2;
        const mx = (x1 + x2) / 2;
        path = `M ${x1} ${y1} C ${mx} ${y1}, ${mx} ${y2}, ${x2} ${y2}`;
      } else if (span > 1) {
        // Skips a column: an arc over the boxes, into the top of the target.
        // It leaves from the right side, so it rises in the gap between columns.
        const x1 = from.x + BW, y1 = from.y + BH / 2, x2 = to.x + BW / 2 - 6, y2 = to.y - 2;
        const lift = top - TOP + 4;
        path = `M ${x1} ${y1} C ${x1 + GX / 2} ${lift}, ${x2} ${lift}, ${x2} ${y2}`;
      } else {
        // Back to an earlier (or the same) column: an arc under the boxes.
        const x1 = from.x + BW / 2 + 6, y1 = from.y + BH, x2 = to.x + BW / 2 - 6, y2 = to.y + BH + 2;
        const dip = boxesBottom + TOP + 14 + backIndex++ * 12;
        path = `M ${x1} ${y1} C ${x1} ${dip}, ${x2} ${dip}, ${x2} ${y2}`;
      }
      const tip = both
        ? `${input} and ${d.column} can each be computed from the other: ${d.column} = ${d.formula}, ${input} = ${byColumn.get(input)!.formula}`
        : `${input} is used in ${d.column} = ${d.formula}`;
      const attrs: Record<string, string> = { d: path, class: both ? "edge both" : "edge", "marker-end": "url(#arrow)", "data-from": input, "data-to": d.column };
      if (both) attrs["marker-start"] = "url(#arrow)";
      graph.append(svg("path", attrs, title(tip)));
    }
  }
  for (const [n, p] of pos) {
    const d = byColumn.get(n);
    const label = n.length > 16 ? `${n.slice(0, 15)}…` : n;
    graph.append(
      svg(
        "g",
        { class: d ? "node derived" : "node", "data-column": n },
        svg("rect", { x: p.x, y: p.y, width: BW, height: BH, rx: 6 }),
        svg("text", { x: p.x + BW / 2, y: p.y + BH / 2 + 4, "text-anchor": "middle" }, label),
        title(d ? `${n} = ${d.formula}` : `${n}: used by other columns`),
      ),
    );
  }
  return graph;
}
