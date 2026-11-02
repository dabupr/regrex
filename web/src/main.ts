import { Engine } from "./engine";
import type { DatasetInfo, Derived } from "./types";

const engine = new Engine();

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const ui = {
  start: $("start"),
  drop: $<HTMLLabelElement>("drop"),
  file: $<HTMLInputElement>("file"),
  example: $<HTMLButtonElement>("example"),
  error: $("error"),
  data: $("data"),
  fileName: $("file-name"),
  fileSummary: $("file-summary"),
  sheetWrap: $("sheet-wrap"),
  sheet: $<HTMLSelectElement>("sheet"),
  scan: $<HTMLButtonElement>("scan"),
  preview: $<HTMLTableElement>("preview"),
  progress: $("progress"),
  meter: $("meter-fill"),
  progressText: $("progress-text"),
  results: $("results"),
  cards: $("cards"),
  independent: $("independent"),
  download: $<HTMLButtonElement>("download"),
};

interface State {
  name: string;
  bytes: ArrayBuffer;
  info: DatasetInfo;
  ignored: Set<string>;
  derived: Derived[];
}
let state: State | undefined;

/** Creates an element. Text is always set with textContent (file contents are untrusted). */
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

function showError(message: string | null) {
  ui.error.hidden = message === null;
  ui.error.textContent = message ?? "";
}

/** Columns that look like identifiers: whole numbers, all different, named like an ID. */
function looksLikeId(name: string, info: DatasetInfo): boolean {
  return /(^id$|_id$|^id_|number|^no\.?$|invoice|code|ref)/i.test(name) && info.columns.some((c) => c.name === name && c.decimals === 0);
}

async function openFile(name: string, bytes: ArrayBuffer, sheet?: string) {
  showError(null);
  try {
    const info = await engine.load(name, bytes, sheet);
    if (info.columns.length === 0) throw new Error("No numeric columns found in this file.");
    const ignored = new Set(info.columns.map((c) => c.name).filter((n) => looksLikeId(n, info)));
    state = { name, bytes, info, ignored, derived: [] };
    renderData();
  } catch (e) {
    showError(`Could not read ${name}: ${(e as Error).message}`);
  }
}

function renderData() {
  if (!state) return;
  const { info, name } = state;
  ui.start.hidden = true;
  ui.results.hidden = true;
  ui.data.hidden = false;
  ui.fileName.textContent = name;
  const text = info.skipped.length ? `, ${info.skipped.length} text` : "";
  ui.fileSummary.textContent = `${info.rows} rows, ${info.columns.length} numeric columns${text}`;

  ui.sheetWrap.hidden = info.sheets.length < 2;
  ui.sheet.replaceChildren(...info.sheets.map((s) => el("option", { value: s }, s)));

  const numeric = new Set(info.columns.map((c) => c.name));
  const head = el("tr", {}, el("th", { class: "text" }, "#"));
  for (const col of info.header) {
    if (!numeric.has(col)) {
      head.append(el("th", { class: "text" }, col, el("span", { class: "kind" }, "text, skipped")));
      continue;
    }
    const box = el("input", { type: "checkbox", "data-column": col });
    box.checked = !state.ignored.has(col);
    box.addEventListener("change", () => {
      if (box.checked) state!.ignored.delete(col);
      else state!.ignored.add(col);
    });
    head.append(el("th", {}, el("label", {}, box, col), el("span", { class: "kind" }, "number")));
  }
  const rows = info.preview.map((cells, i) =>
    el(
      "tr",
      { "data-row": String(i + 1) },
      el("td", { class: "rownum" }, String(i + 1)),
      ...info.header.map((col, c) => el("td", numeric.has(col) ? {} : { class: "text" }, cells[c] ?? "")),
    ),
  );
  ui.preview.replaceChildren(el("thead", {}, head), el("tbody", {}, ...rows));
}

async function scanAll() {
  if (!state) return;
  const columns = state.info.columns.map((c) => c.name).filter((n) => !state!.ignored.has(n));
  const ignore = [...state.ignored];
  ui.scan.disabled = true;
  ui.progress.hidden = false;
  ui.results.hidden = true;
  state.derived = [];
  try {
    for (const [i, column] of columns.entries()) {
      ui.progressText.textContent = `Checking ${column} (${i + 1} of ${columns.length})…`;
      ui.meter.style.width = `${(100 * i) / columns.length}%`;
      const found = await engine.explain(column, ignore);
      if (found) state.derived.push(found);
    }
    ui.meter.style.width = "100%";
    renderResults(columns);
  } catch (e) {
    showError(`Something went wrong: ${(e as Error).message}`);
  } finally {
    ui.scan.disabled = false;
    ui.progress.hidden = true;
  }
}

function formulaNode(d: Derived): HTMLElement {
  return el("div", { class: "formula" }, el("b", {}, d.column), ` = ${d.formula}`);
}

function renderResults(columns: string[]) {
  if (!state) return;
  ui.results.hidden = false;
  const cards = state.derived.map((d) => {
    const exceptions = d.exception_rows.length;
    const status =
      exceptions === 0
        ? el("span", { class: "ok" }, `✓ all ${d.checked_rows} rows`)
        : el("span", { class: "warn" }, `${d.matched_rows} of ${d.checked_rows} rows`);
    const card = el(
      "article",
      { class: "card", "data-column": d.column },
      formulaNode(d),
      el("div", { class: "meta" }, status, el("span", { class: "muted" }, `uses ${d.inputs.join(", ")}`)),
    );
    if (exceptions > 0) {
      const rows = d.exceptions.map((x) =>
        el("tr", {}, el("td", {}, String(x.row)), el("td", {}, fmt(x.actual)), el("td", {}, fmt(x.expected))),
      );
      const more = exceptions > d.exceptions.length ? ` (first ${d.exceptions.length} shown)` : "";
      card.append(
        el(
          "details",
          {},
          el("summary", {}, `${exceptions === 1 ? "1 row breaks" : `${exceptions} rows break`} the rule${more}`),
          el(
            "table",
            {},
            el("thead", {}, el("tr", {}, el("th", {}, "Row"), el("th", {}, "In the file"), el("th", {}, "Formula gives"))),
            el("tbody", {}, ...rows),
          ),
        ),
      );
      highlight(d.exception_rows);
    }
    return card;
  });
  ui.cards.replaceChildren(
    ...(cards.length ? cards : [el("p", {}, "No column could be computed from the others.")]),
  );
  const found = new Set(state.derived.map((d) => d.column));
  const rest = columns.filter((c) => !found.has(c));
  ui.independent.textContent = rest.length ? `No formula found for: ${rest.join(", ")}.` : "";
}

function highlight(rows: number[]) {
  for (const r of rows) ui.preview.querySelector(`tr[data-row="${r}"]`)?.classList.add("bad");
}

function fmt(x: number): string {
  return Number.isInteger(x) ? String(x) : String(Number(x.toPrecision(8)));
}

function download() {
  if (!state) return;
  const report = {
    schema_version: 1,
    file: state.name,
    rows: state.info.rows,
    columns: state.info.columns.map((c) => c.name),
    skipped_columns: state.info.skipped,
    derived: state.derived,
  };
  const blob = new Blob([JSON.stringify(report, null, 2)], { type: "application/json" });
  const a = el("a", { href: URL.createObjectURL(blob), download: `${state.name.replace(/\.[^.]+$/, "")}.regrex.json` });
  a.click();
  URL.revokeObjectURL(a.href);
}

async function readFile(file: File) {
  await openFile(file.name, await file.arrayBuffer());
}

ui.file.addEventListener("change", () => {
  const f = ui.file.files?.[0];
  if (f) void readFile(f);
});
ui.drop.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") ui.file.click();
});
for (const type of ["dragenter", "dragover"]) {
  ui.drop.addEventListener(type, (e) => {
    e.preventDefault();
    ui.drop.classList.add("over");
  });
}
for (const type of ["dragleave", "drop"]) {
  ui.drop.addEventListener(type, () => ui.drop.classList.remove("over"));
}
ui.drop.addEventListener("drop", (e) => {
  e.preventDefault();
  const f = (e as DragEvent).dataTransfer?.files[0];
  if (f) void readFile(f);
});
ui.example.addEventListener("click", async () => {
  const res = await fetch(`${import.meta.env.BASE_URL}invoices.csv`);
  await openFile("invoices.csv", await res.arrayBuffer());
});
ui.sheet.addEventListener("change", () => {
  if (state) void openFile(state.name, state.bytes, ui.sheet.value);
});
ui.scan.addEventListener("click", () => void scanAll());
ui.download.addEventListener("click", download);
