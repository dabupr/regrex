import { dependencyGraph, scatter, scatterLegend } from "./charts";
import { Engine } from "./engine";
import { columnarFormat, readColumnar } from "./formats";
import type { DatasetInfo, Derived, Known } from "./types";
import {
  ADVANCED,
  DEFAULTS,
  OPERATORS,
  defaultLabel,
  describe,
  loadSettings,
  saveSettings,
  searchOptions,
  type Profile,
  type Settings,
  type Source,
} from "./settings";

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
  sheetLabel: $("sheet-label"),
  sheet: $<HTMLSelectElement>("sheet"),
  scan: $<HTMLButtonElement>("scan"),
  preview: $<HTMLTableElement>("preview"),
  progress: $("progress"),
  meter: $("meter-fill"),
  progressText: $("progress-text"),
  results: $("results"),
  graph: $<HTMLDetailsElement>("graph"),
  graphWrap: $("graph-wrap"),
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
  advancedFields: $("advanced-fields"),
  domain: $<HTMLSelectElement>("domain"),
  stop: $<HTMLButtonElement>("stop"),
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
  /** The sheet or table being shown, when the file has several. */
  sheet?: string;
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

/** Loads one sheet (or SQLite table) of a file into the engine. */
async function loadSheet(name: string, bytes: ArrayBuffer, sheet?: string): Promise<DatasetInfo> {
  const format = columnarFormat(name, bytes);
  if (!format) return engine.load(name, bytes, sheet);
  // Read in the page (the reader is loaded on demand), then handed over as JSON.
  const table = await readColumnar(format, bytes, sheet);
  const info = await engine.load(`${name}.json`, table.json);
  return { ...info, sheets: table.sheets };
}

async function openFile(name: string, bytes: ArrayBuffer, sheet?: string) {
  showError(null);
  try {
    let info = await loadSheet(name, bytes, sheet);
    // Without a choice, skip sheets or tables with no numbers (a list of customers).
    if (info.columns.length === 0 && sheet === undefined) {
      for (const other of info.sheets.slice(1)) {
        const next = await loadSheet(name, bytes, other);
        if (next.columns.length > 0) {
          info = next;
          sheet = other;
          break;
        }
      }
    }
    if (info.columns.length === 0) throw new Error("No numeric columns found in this file.");
    const ignored = new Set(info.columns.map((c) => c.name).filter((n) => looksLikeId(n, info)));
    state = { name, bytes, info, ignored, derived: [], suspects: [], sheet };
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
  ui.sheetLabel.textContent = columnarFormat(name, state.bytes) === "sqlite" ? "Table" : "Sheet";
  ui.sheet.replaceChildren(...info.sheets.map((s) => el("option", { value: s }, s)));
  if (state.sheet && info.sheets.includes(state.sheet)) ui.sheet.value = state.sheet;

  const numeric = new Set(info.columns.map((c) => c.name));
  const readAs = new Map(info.columns.map((c) => [c.name, c.read_as]));
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
    const kind = readAs.get(col) ?? "number";
    head.append(el("th", {}, el("label", {}, box, col), el("span", { class: "kind" }, kind)));
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
  for (const radio of document.querySelectorAll<HTMLInputElement>('input[name="source"]')) {
    radio.checked = radio.value === settings.source;
  }
  const library = settings.source === "library";
  ui.domain.disabled = !library;
  ui.domain.value = settings.domain;
  for (const part of document.querySelectorAll<HTMLElement>(".search-only")) part.hidden = library;
  for (const radio of document.querySelectorAll<HTMLInputElement>('input[name="profile"]')) {
    radio.checked = radio.value === settings.profile;
  }
  for (const box of ui.operators.querySelectorAll<HTMLInputElement>("input")) {
    box.checked = settings.operators.includes(box.value);
  }
  for (const select of ui.advancedFields.querySelectorAll<HTMLSelectElement>("select")) {
    select.value = String(settings.advanced[select.name as keyof Settings["advanced"]]);
  }
  const domainName = ui.domain.selectedOptions[0]?.value ? ui.domain.selectedOptions[0].textContent ?? undefined : undefined;
  ui.settingsSummary.textContent = `(${describe(settings, domainName)})`;
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
  const { name, bytes } = state;
  const sheet = state.info.sheets.length > 1 ? ui.sheet.value : undefined;
  // Stopping (by hand or at the time limit) replaces the engine's worker.
  let stopped: "user" | "time" | undefined;
  const stop = (why: "user" | "time") => {
    if (stopped) return;
    stopped = why;
    engine.restart();
  };
  scanStop = () => stop("user");
  const limit = settings.advanced.time_limit;
  const timer = limit > 0 ? setTimeout(() => stop("time"), limit * 1000) : undefined;
  let checked = 0;
  try {
    for (const [i, column] of columns.entries()) {
      ui.progressText.textContent = `Checking ${column} (${i + 1} of ${columns.length})…`;
      ui.meter.style.width = `${(100 * i) / columns.length}%`;
      const found = await engine.explain(column, options);
      if (found) state.derived.push(found);
      checked++;
    }
    ui.meter.style.width = "100%";
    if (state.derived.length > 1) state.derived = await engine.review(ignore);
  } catch (e) {
    if (!stopped) showError(`Something went wrong: ${(e as Error).message}`);
  } finally {
    clearTimeout(timer);
    scanStop = undefined;
  }
  try {
    // The new worker has no file yet. The formulas found so far are kept,
    // only diagnosed one by one instead of together.
    if (stopped) await engine.load(name, bytes, sheet);
    if (state.name === name) {
      state.suspects = await findSuspects(state.derived);
      renderResults(columns.slice(0, checked), stopped && { why: stopped, skipped: columns.slice(checked) });
      renderSuspects();
    }
  } catch (e) {
    showError(`Something went wrong: ${(e as Error).message}`);
  } finally {
    ui.scan.disabled = false;
    ui.progress.hidden = true;
  }
}

/** Stops the running scan, if any. */
let scanStop: (() => void) | undefined;

/** Whether the rows that break a formula are few enough to be mistakes. */
function fewExceptions(d: Derived): boolean {
  const wrong = wrongRows(d).length;
  return wrong > 0 && wrong <= MISTAKE_SHARE * d.checked_rows;
}

/** Rows that follow neither the formula nor the column's other formula. */
function wrongRows(d: Derived): number[] {
  const other = new Set(d.other?.rows ?? []);
  return d.exception_rows.filter((r) => !other.has(r));
}

/** Rows that break a formula followed by almost every other row, with their cells. */
async function findSuspects(derived: Derived[]): Promise<Suspect[]> {
  const reasons = new Map<number, Suspect["reasons"]>();
  for (const d of derived.filter(fewExceptions)) {
    const details = new Map(d.exceptions.map((x) => [x.row, x]));
    for (const row of wrongRows(d)) {
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
  const segments = state.derived.filter((d) => !d.other && wrongRows(d).length > 0 && !fewExceptions(d));
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
  const wrong = wrongRows(d).length;
  const status =
    exceptions === 0
      ? el("span", { class: "ok" }, `✓ all ${d.checked_rows} rows`)
      : d.other
        ? el("span", { class: wrong ? "warn" : "ok" }, `${d.matched_rows} of ${d.checked_rows} rows${wrong ? `, ${wrong} with neither formula` : ", the rest with a second formula"}`)
        : el("span", { class: "warn" }, `${d.matched_rows} of ${d.checked_rows} rows`);
  const card = el(
    "article",
    { class: "card", "data-column": d.column },
    formulaNode(d),
    el("div", { class: "meta" }, status, el("span", { class: "muted" }, `uses ${d.inputs.join(", ")}`)),
  );
  if (d.known) card.append(knownNode(d.known));
  if (d.other) {
    const o = d.other;
    const n = o.rows.length === 1 ? "1 row" : `${o.rows.length} rows`;
    card.append(
      el(
        "div",
        { class: "other" },
        el("span", { class: "badge other-badge" }, "Second formula"),
        " ",
        el("span", { class: "formula-inline" }, el("b", {}, d.column), ` = ${o.formula}`),
        el("span", { class: "muted" }, o.split ? ` for ${n}, where ${o.split.rule}` : ` for ${n}`),
      ),
    );
    if (!o.split) {
      card.append(el("p", { class: "note" }, "No column tells these rows apart; a text column such as a product or region may."));
    }
  } else if (exceptions > 0 && !fewExceptions(d) && d.matched_rows >= 0.5 * d.checked_rows) {
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
        el(
          "summary",
          {},
          d.other
            ? `${exceptions} rows follow another formula${wrong ? `, ${wrong} of them neither` : ""}${more}`
            : `${exceptions === 1 ? "1 row breaks" : `${exceptions} rows break`} the rule${more}`,
        ),
        el(
          "table",
          {},
          el("thead", {}, el("tr", {}, el("th", {}, "Row"), el("th", {}, "In the file"), el("th", {}, "Formula gives"), el("th", {}, "Likely mistake"))),
          el("tbody", {}, ...rows),
        ),
      ),
    );
  }
  card.append(chartToggle(d));
  return card;
}

/** A "Chart" section that plots the file against the formula when opened. */
function chartToggle(d: Derived): HTMLElement {
  const body = el("div", { class: "chart-body" });
  const details = el("details", { class: "chart-toggle" }, el("summary", {}, "Chart: file against formula"), body);
  details.addEventListener("toggle", async () => {
    if (!details.open || body.childElementCount > 0) return;
    body.append(el("p", { class: "muted" }, "Drawing…"));
    try {
      const values = await engine.values(d.column, d.formula);
      body.replaceChildren(scatter(d, values), scatterLegend(d));
    } catch (e) {
      body.replaceChildren(el("p", { class: "muted" }, `No chart for this formula: ${(e as Error).message}`));
    }
  });
  return details;
}

/** The dependency graph of the columns found, with each computed column linked to its card. */
function renderGraph(derived: Derived[]) {
  const graph = dependencyGraph(derived);
  ui.graph.hidden = graph === null;
  if (!graph) return ui.graphWrap.replaceChildren();
  for (const node of graph.querySelectorAll<SVGGElement>(".node.derived")) {
    node.setAttribute("tabindex", "0");
    node.setAttribute("role", "link");
    const go = () => {
      const card = ui.cards.querySelector<HTMLElement>(`article[data-column="${CSS.escape(node.dataset.column ?? "")}"]`);
      card?.scrollIntoView({ behavior: "smooth", block: "center" });
      card?.classList.remove("flash");
      void card?.offsetWidth; // restart the animation
      card?.classList.add("flash");
    };
    node.addEventListener("click", go);
    node.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        go();
      }
    });
  }
  ui.graphWrap.replaceChildren(graph);
}

function renderResults(columns: string[], stopped?: { why: "user" | "time"; skipped: string[] }) {
  if (!state) return;
  ui.results.hidden = false;
  const cards = state.derived.map(derivedCard);
  for (const d of state.derived) highlight(d.exception_rows);
  ui.cards.replaceChildren(
    ...(cards.length ? cards : [el("p", {}, "No column could be computed from the others.")]),
  );
  renderGraph(state.derived);
  const found = new Set(state.derived.map((d) => d.column));
  const rest = columns.filter((c) => !found.has(c));
  const notes = rest.length ? [`No formula found for: ${rest.join(", ")}.`] : [];
  if (stopped) {
    const why = stopped.why === "time" ? "The time limit was reached" : "The search was stopped";
    notes.push(`${why} before checking: ${stopped.skipped.join(", ")}.`);
  }
  ui.independent.textContent = notes.join(" ");
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
// Cells copied from a spreadsheet arrive as tab-separated text.
document.addEventListener("paste", (e) => {
  if (e.target instanceof Element && e.target.closest("input, textarea, select, [contenteditable]")) return;
  const text = e.clipboardData?.getData("text/plain") ?? "";
  if (!text.trim()) return;
  e.preventDefault();
  void openFile("pasted cells", new TextEncoder().encode(text).buffer as ArrayBuffer);
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
ui.reset.addEventListener("click", () => updateSettings(structuredClone(DEFAULTS)));
ui.stop.addEventListener("click", () => scanStop?.());
for (const radio of document.querySelectorAll<HTMLInputElement>('input[name="source"]')) {
  radio.addEventListener("change", () => updateSettings({ source: radio.value as Source }));
}
ui.domain.addEventListener("change", () => updateSettings({ domain: ui.domain.value }));
void engine.domains().then((domains) => {
  ui.domain.append(...domains.map((d) => el("option", { value: d.id, title: d.description }, d.name)));
  // A saved domain that no longer exists falls back to every domain.
  if (!domains.some((d) => d.id === settings.domain)) settings.domain = "";
  renderSettings();
});

ui.advancedFields.replaceChildren(
  ...ADVANCED.map((a) => {
    const id = `opt-${a.key}`;
    const select = el("select", { id, name: a.key, "aria-describedby": `${id}-help` });
    select.append(...a.choices.map((c) => el("option", { value: String(c.value) }, c.label)));
    select.addEventListener("change", () =>
      updateSettings({ advanced: { ...settings.advanced, [a.key]: Number(select.value) } }),
    );
    return el(
      "div",
      { class: "field" },
      el("label", { for: id }, a.label, select),
      el("p", { id: `${id}-help`, class: "hint muted" }, `${a.help} `, el("span", { class: "default" }, `Default: ${defaultLabel(a.key)}.`)),
    );
  }),
);
ui.ownForm.addEventListener("submit", (e) => {
  e.preventDefault();
  void checkOwnFormula();
});
renderSettings();
