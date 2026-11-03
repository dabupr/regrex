// The "Library" page: every formula Regrex recognises, with search and a
// domain filter. Runs the WebAssembly engine on the main thread, since
// listing the library is quick.

import init, { library } from "./wasm/regrex_wasm.js";
import type { Library, LibraryFormula, LibraryQuantity } from "./types";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const ui = {
  q: $<HTMLInputElement>("q"),
  domain: $<HTMLSelectElement>("domain"),
  count: $("count"),
  list: $("list"),
};

/** Creates an element. Text is always set with textContent. */
function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<Record<string, string>> = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined) continue;
    if (k === "class") node.className = v;
    else node.setAttribute(k, v);
  }
  node.append(...children);
  return node;
}

/** Lowercase and without accents, so "fisica" finds "física". */
function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

function quantity(q: LibraryQuantity): HTMLElement {
  const unit = q.unit && q.unit !== "1" ? el("span", { class: "muted" }, ` (${q.unit})`) : "";
  return el("li", {}, el("code", {}, q.symbol), ` ${q.name}`, unit);
}

function card(f: LibraryFormula, domainName: string): HTMLElement {
  const source = f.source_url
    ? el("a", { href: f.source_url, rel: "noopener", target: "_blank" }, f.source_title)
    : el("span", {}, f.source_title);
  return el(
    "article",
    { class: "card lib-card", id: f.id },
    el("h2", {}, f.name),
    el("p", { class: "muted" }, domainName, f.tags.length ? ` · ${f.tags.join(", ")}` : ""),
    el("p", { class: "formula" }, `${f.output.symbol} = ${f.expression}`),
    f.description ? el("p", {}, f.description) : "",
    el("ul", { class: "vars" }, quantity(f.output), ...f.variables.map(quantity)),
    el("p", { class: "muted" }, "Source: ", source),
  );
}

async function main() {
  await init();
  const lib = JSON.parse(library()) as Library;
  const domains = new Map(lib.domains.map((d) => [d.id, d.name]));
  ui.domain.append(...lib.domains.map((d) => el("option", { value: d.id }, d.name)));
  const cards = lib.formulas.map((f) => {
    const text = fold(
      [f.id, f.name, f.description ?? "", f.expression, f.domain, ...f.tags, f.output.name, ...f.variables.map((v) => v.name)].join(" "),
    );
    return { f, text, node: card(f, domains.get(f.domain) ?? f.domain) };
  });
  ui.list.replaceChildren(...cards.map((c) => c.node));

  const filter = () => {
    const words = fold(ui.q.value).split(/\s+/).filter(Boolean);
    const domain = ui.domain.value;
    let shown = 0;
    for (const c of cards) {
      const visible = (!domain || c.f.domain === domain) && words.every((w) => c.text.includes(w));
      c.node.hidden = !visible;
      if (visible) shown++;
    }
    ui.count.textContent = `${shown} of ${cards.length} formulas`;
  };
  ui.q.addEventListener("input", filter);
  ui.domain.addEventListener("change", filter);
  filter();

  // A link such as library.html#kinetic_energy opens on that formula.
  const target = location.hash ? document.getElementById(decodeURIComponent(location.hash.slice(1))) : null;
  if (target) {
    target.classList.add("target");
    target.scrollIntoView();
  }
}

main().catch((e: unknown) => {
  ui.count.textContent = `Could not load the library: ${e instanceof Error ? e.message : String(e)}`;
});
