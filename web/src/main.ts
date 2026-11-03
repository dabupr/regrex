import { Engine } from "./engine";
import type { DatasetInfo, Derived, Known } from "./types";
import { DEFAULTS, OPERATORS, describe, loadSettings, saveSettings, searchOptions, type Profile, type Settings } from "./settings";

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
  summary: $("summary"),
  suspicious: $("suspicious"),
  suspiciousTable: $<HTMLTableElement>("suspicious-table"),
  export: $<HTMLButtonElement>("export"),
  settingsSummary: $("settings-summary"),
  only: $<HTMLSelectElement>("only"),
  operators: $("operators"),
  reset: $<HTMLButtonElement>("reset"),
  ownForm: $<HTMLFormElement>("own-form"),
  ownColumn: $<HTMLSelectElement>("own-column"),
  ownFormula: $<HTMLInputElement>("own-formula"),
  ownError: $("own-error"),
  ownResult: $("own-result"),
};

let settings: Settings = loadSettings();

/**
 * When at most this share of the rows break a formula, they are treated as
 * mistakes. A larger group is more likely a separate kind of row that follows
 * another rule (for example another tax rate), not an error.
 */
const MISTAKE_SHARE = 0.05;

/** A suspicious row: its number, its cells as text and why it is suspicious. */
interface Suspect {
  row: number;
  cells: string[];
  /** `blamed` is false when the column only breaks because another cell is wrong. */
  reasons: { column: string; text: string; blamed: boolean }[];
}

interface State {
  name: string;
  bytes: ArrayBuffer;
  info: DatasetInfo;
  ignored: Set<string>;
  derived: Derived[];
  suspects: Suspect[];
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
    state = { name, bytes, info, ignored, derived: [], suspects: [] };
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
  ui.suspicious.hidden = true;
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
      renderColumnChoices();
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
  renderColumnChoices();
  ui.ownResult.replaceChildren();
  showOwnError(null);
}

/** Fills the column choices of the settings and of "Test your own formula". */
function renderColumnChoices() {
  if (!state) return;
  const usable = state.info.columns.map((c) => c.name).filter((n) => !state!.ignored.has(n));
  const only = ui.only.value;
  ui.only.replaceChildren(
    el("option", { value: "" }, "every column"),
    ...usable.map((c) => el("option", { value: c }, `only ${c}`)),
  );
  ui.only.value = usable.includes(only) ? only : "";
  const own = ui.ownColumn.value;
  const all = state.info.columns.map((c) => c.name);
  ui.ownColumn.replaceChildren(...all.map((c) => el("option", { value: c }, c)));
  if (all.includes(own)) ui.ownColumn.value = own;
}

function renderSettings() {
  for (const radio of document.querySelectorAll<HTMLInputElement>('input[name="profile"]')) {
    radio.checked = radio.value === settings.profile;
  }
  for (const box of ui.operators.querySelectorAll<HTMLInputElement>("input")) {
    box.checked = settings.operators.includes(box.value);
  }
  ui.settingsSummary.textContent = `(${describe(settings)})`;
}

function updateSettings(change: Partial<Settings>) {
  settings = { ...settings, ...change };
  saveSettings(settings);
  renderSettings();
}

function showOwnError(message: string | null) {
  ui.ownError.hidden = message === null;
  ui.ownError.textContent = message ?? "";
}

async function checkOwnFormula() {
  if (!state) return;
  const column = ui.ownColumn.value;
  // "vat = rate * net" is accepted as well as "rate * net".
  let text = ui.ownFormula.value.trim();
  if (text.startsWith(column) && text.slice(column.length).trimStart().startsWith("=")) {
    text = text.slice(column.length).trimStart().slice(1);
  }
  showOwnError(null);
  ui.ownResult.replaceChildren(el("p", { class: "muted" }, "Checking…"));
  try {
    const d = await engine.formula(column, text);
    ui.ownResult.replaceChildren(derivedCard(d));
  } catch (e) {
    ui.ownResult.replaceChildren();
    showOwnError((e as Error).message);
  }
}

async function scanAll() {
  if (!state) return;
  const usable = state.info.columns.map((c) => c.name).filter((n) => !state!.ignored.has(n));
  const columns = usable.includes(ui.only.value) ? [ui.only.value] : usable;
  const ignore = [...state.ignored];
  const options = { ignore, ...searchOptions(settings) };
  ui.scan.disabled = true;
  ui.progress.hidden = false;
  ui.results.hidden = true;
  ui.suspicious.hidden = true;
  state.derived = [];
  state.suspects = [];
  try {
    for (const [i, column] of columns.entries()) {
      ui.progressText.textContent = `Checking ${column} (${i + 1} of ${columns.length})…`;
      ui.meter.style.width = `${(100 * i) / columns.length}%`;
      const found = await engine.explain(column, options);
      if (found) state.derived.push(found);
    }
    ui.meter.style.width = "100%";
    if (state.derived.length > 1) state.derived = await engine.review(ignore);
    state.suspects = await findSuspects(state.derived);
    renderResults(columns);
    renderSuspects();
  } catch (e) {
    showError(`Something went wrong: ${(e as Error).message}`);
  } finally {
    ui.scan.disabled = false;
    ui.progress.hidden = true;
  }
}

/** Whether the rows that break a formula are few enough to be mistakes. */
function fewExceptions(d: Derived): boolean {
  return d.exception_rows.length > 0 && d.exception_rows.length <= MISTAKE_SHARE * d.checked_rows;
}

/** Rows that break a formula followed by almost every other row, with their cells. */
async function findSuspects(derived: Derived[]): Promise<Suspect[]> {
  const reasons = new Map<number, Suspect["reasons"]>();
  for (const d of derived.filter(fewExceptions)) {
    const details = new Map(d.exceptions.map((x) => [x.row, x]));
    for (const row of d.exception_rows) {
      const x = details.get(row);
      const why = x
        ? x.diagnosis?.kind === "other_cell"
          ? `${d.column}: ${x.diagnosis.description}`
          : `${d.column} is ${fmt(x.actual)}, the formula gives ${fmt(x.expected)}${x.diagnosis ? `: ${x.diagnosis.description}` : ""}`
        : `${d.column} breaks ${d.column} = ${d.formula}`;
      const blamed = x?.diagnosis?.kind !== "other_cell";
      reasons.set(row, [...(reasons.get(row) ?? []), { column: d.column, text: why, blamed }]);
    }
  }
  const rows = [...reasons.keys()].sort((a, b) => a - b);
  const cells = await engine.rows(rows);
  return rows.map((row, i) => ({ row, cells: cells[i] ?? [], reasons: reasons.get(row)! }));
}

function renderSuspects() {
  if (!state) return;
  const { suspects, info } = state;
  const n = suspects.length;
  const segments = state.derived.filter((d) => d.exception_rows.length > 0 && !fewExceptions(d));
  const found = state.derived.length;
  const parts: (Node | string)[] = [
    el("span", {}, found === 1 ? "1 formula found" : `${found} formulas found`),
  ];
  if (n === 0) {
    parts.push(el("span", { class: "ok" }, "✓ No suspicious rows"));
  } else {
    const link = el("a", { href: "#suspicious" }, "See them");
    parts.push(el("span", { class: "warn" }, `⚠ ${n === 1 ? "1 suspicious row" : `${n} suspicious rows`}`), link);
  }
  if (segments.length) {
    const names = segments.map((d) => d.column).join(", ");
    parts.push(el("span", { class: "muted" }, `Some rows of ${names} follow another rule; see below.`));
  }
  ui.summary.replaceChildren(...parts);
  ui.summary.classList.toggle("has-warn", n > 0);

  ui.suspicious.hidden = n === 0;
  if (n === 0) return;
  const columnIndex = new Map(info.header.map((h, i) => [headerName(h), i]));
  const head = el("tr", {}, el("th", { class: "text" }, "Row"), ...info.header.map((h) => el("th", {}, h)), el("th", { class: "text" }, "Why"));
  const body = suspects.map((s) => {
    const bad = new Set(s.reasons.filter((r) => r.blamed).map((r) => columnIndex.get(r.column)));
    return el(
      "tr",
      { "data-row": String(s.row) },
      el("td", { class: "rownum" }, String(s.row)),
      ...info.header.map((_, c) => el("td", { class: bad.has(c) ? "bad-cell" : "" }, s.cells[c] ?? "")),
      el("td", { class: "text why" }, ...s.reasons.flatMap((r, i) => (i ? [el("br"), r.text] : [r.text]))),
    );
  });
  ui.suspiciousTable.replaceChildren(el("thead", {}, head), el("tbody", {}, ...body));
}

/** The column name Regrex uses for a header: `speed (m/s)` is `speed`. */
function headerName(header: string): string {
  return header.replace(/\s*[([].*[)\]]\s*$/, "").trim();
}

/** A CSV cell, quoted when needed and safe to open in a spreadsheet. */
function csvCell(text: string): string {
  // A cell starting with = + - @ could run as a formula in a spreadsheet;
  // numbers such as -5 are kept as they are.
  const risky = /^[=@\t\r]/.test(text) || (/^[+-]/.test(text) && !/^[+-]?[\d.,]+%?$/.test(text));
  const safe = risky ? `'${text}` : text;
  return /[",\n\r]/.test(safe) ? `"${safe.replace(/"/g, '""')}"` : safe;
}

function exportSuspects() {
  if (!state || state.suspects.length === 0) return;
  const lines = [["row", ...state.info.header, "why"]];
  for (const s of state.suspects) {
    lines.push([String(s.row), ...state.info.header.map((_, c) => s.cells[c] ?? ""), s.reasons.map((r) => r.text).join("; ")]);
  }
  const csv = lines.map((l) => l.map(csvCell).join(",")).join("\r\n") + "\r\n";
  const blob = new Blob([csv], { type: "text/csv" });
  const a = el("a", { href: URL.createObjectURL(blob), download: `${state.name.replace(/\.[^.]+$/, "")}.suspicious.csv` });
  a.click();
  URL.revokeObjectURL(a.href);
}

function formulaNode(d: Derived): HTMLElement {
  return el("div", { class: "formula" }, el("b", {}, d.column), ` = ${d.formula}`);
}

/** A "Known formula" badge that opens a card about the library formula. */
function knownNode(k: Known): HTMLElement {
  const bindings = k.variables.map((v) => {
    const where = v.column !== null ? el("b", {}, v.column) : el("b", {}, fmt(v.value ?? 0));
    const unit = v.unit && v.unit !== "1" ? ` (${v.unit})` : "";
    return el("li", {}, el("code", {}, v.symbol), ` ${v.name}${unit}: `, where);
  });
  const output = k.output.column ?? "";
  const body = el(
    "div",
    { class: "known-card" },
    el("p", {}, el("strong", {}, k.name), el("span", { class: "muted" }, ` · ${k.domain}`)),
    k.description ? el("p", {}, k.description) : "",
    el("p", {}, el("code", {}, `${k.output.symbol} = ${k.expression}`)),
    el("ul", {}, el("li", {}, el("code", {}, k.output.symbol), ` ${k.output.name}: `, el("b", {}, output)), ...bindings),
  );
  if (k.changed_constant) {
    const c = k.changed_constant;
    body.append(
      el(
        "p",
        { class: "warn" },
        `The data uses ${c.id} = ${fmt(c.value)} instead of the usual ${fmt(c.library_value)}.`,
      ),
    );
  }
  const source = k.source_url
    ? el("a", { href: k.source_url, rel: "noopener", target: "_blank" }, k.source_title)
    : el("span", {}, k.source_title);
  body.append(
    el(
      "p",
      { class: "muted" },
      "Source: ",
      source,
      " · ",
      el("a", { href: `library.html#${encodeURIComponent(k.id)}` }, "See it in the library"),
    ),
  );
  return el("details", { class: "known" }, el("summary", {}, el("span", { class: "badge" }, "Known formula"), ` ${k.name}`), body);
}

/** The card for one formula: status, inputs, known formula and the rows that break it. */
function derivedCard(d: Derived): HTMLElement {
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
  if (d.known) card.append(knownNode(d.known));
  if (exceptions > 0 && !fewExceptions(d) && d.matched_rows >= 0.5 * d.checked_rows) {
    const share = Math.round((100 * exceptions) / d.checked_rows);
    card.append(
      el(
        "p",
        { class: "note" },
        `${share}% of the rows follow another rule. They may be a separate group (another rate, product or period) rather than mistakes.`,
      ),
    );
  }
  if (exceptions > 0) {
    const rows = d.exceptions.map((x) =>
      el(
        "tr",
        {},
        el("td", {}, String(x.row)),
        el("td", {}, fmt(x.actual)),
        el("td", {}, fmt(x.expected)),
        el("td", { class: "muted" }, x.diagnosis ? x.diagnosis.description : ""),
      ),
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
          el("thead", {}, el("tr", {}, el("th", {}, "Row"), el("th", {}, "In the file"), el("th", {}, "Formula gives"), el("th", {}, "Likely mistake"))),
          el("tbody", {}, ...rows),
        ),
      ),
    );
  }
  return card;
}

function renderResults(columns: string[]) {
  if (!state) return;
  ui.results.hidden = false;
  const cards = state.derived.map(derivedCard);
  for (const d of state.derived) highlight(d.exception_rows);
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
ui.export.addEventListener("click", exportSuspects);

ui.operators.replaceChildren(
  ...OPERATORS.map((o) => el("label", {}, el("input", { type: "checkbox", value: o.id }), ` ${o.label}`)),
);
for (const radio of document.querySelectorAll<HTMLInputElement>('input[name="profile"]')) {
  radio.addEventListener("change", () => updateSettings({ profile: radio.value as Profile }));
}
ui.operators.addEventListener("change", () => {
  const checked = [...ui.operators.querySelectorAll<HTMLInputElement>("input:checked")].map((b) => b.value);
  updateSettings({ operators: checked });
});
ui.reset.addEventListener("click", () => updateSettings({ ...DEFAULTS }));
ui.ownForm.addEventListener("submit", (e) => {
  e.preventDefault();
  void checkOwnFormula();
});
renderSettings();
