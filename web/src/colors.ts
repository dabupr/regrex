// A colour for each column, the same in the preview, the formulas, the
// tables per type and the graph, so a column can be followed at a glance.

/** Colours in the palette (`--c0` … in style.css). */
const PALETTE = 10;

const order = new Map<string, number>();

/** Gives the columns of a newly read file their colours, in file order. */
export function setColumns(names: string[]) {
  order.clear();
  for (const n of names) if (!order.has(n)) order.set(n, order.size);
}

/** The CSS colour of a column: `GIS5550` read as `code#num` shares the colour of `code`. */
export function columnColor(name: string): string {
  const base = name.replace(/#[a-z]+$/, "");
  let i = order.get(name) ?? order.get(base);
  if (i === undefined) {
    i = [...base].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
  }
  return `var(--c${i % PALETTE})`;
}

/** A column name in its colour. */
export function columnName(name: string, tag: "b" | "span" = "b"): HTMLElement {
  const e = document.createElement(tag);
  e.className = "col";
  e.style.setProperty("--col", columnColor(name));
  e.textContent = name;
  return e;
}

/**
 * Formula text with each column name in its colour. Names are matched
 * longest first and only as whole names, so `pk` is not found in `pk2`.
 */
export function colorize(text: string, names: string[]): (Node | string)[] {
  const sorted = [...new Set(names)].filter(Boolean).sort((a, b) => b.length - a.length);
  if (sorted.length === 0) return [text];
  const escaped = sorted.map((n) => n.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  const re = new RegExp(`(?<![\\w#])(${escaped.join("|")})(?![\\w#])`, "g");
  const out: (Node | string)[] = [];
  let last = 0;
  for (const m of text.matchAll(re)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(columnName(m[0], "span"));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}
