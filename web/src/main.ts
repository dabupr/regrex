import { dependencyGraph, scatter, scatterLegend } from "./charts";
import { Engine } from "./engine";
import { groups, loadChoice, saveChoice, suggestions } from "./columns";
import { colorize, columnColor, columnName, setColumns } from "./colors";
import { columnarFormat, readColumnar } from "./formats";
import type { DatasetInfo, Derived, ExplainOptions, Known, NearMiss, SearchEvent, TextRelation } from "./types";
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

// `?workers=1` runs the scan on one worker, to compare timings.
const workersParam = Number(new URLSearchParams(location.search).get("workers"));
const engine = workersParam > 0 ? new Engine(Math.min(16, Math.floor(workersParam))) : new Engine();

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
  progressLive: $("progress-live"),
  almost: $<HTMLDetailsElement>("almost"),
  almostCount: $("almost-count"),
  almostList: $("almost-list"),
  results: $("results"),
  graph: $<HTMLDetailsElement>("graph"),
  graphWrap: $("graph-wrap"),
  cards: $("cards"),
  independent: $("independent"),
  copies: $<HTMLDetailsElement>("copies"),
  copiesCount: $("copies-count"),
  copiesList: $("copies-list"),
  download: $<HTMLButtonElement>("download"),
  summary: $("summary"),
  suspicious: $("suspicious"),
  suspiciousTable: $<HTMLTableElement>("suspicious-table"),
  suspiciousMore: $<HTMLButtonElement>("suspicious-more"),
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
  previewMore: $<HTMLButtonElement>("preview-more"),
  columnsPanel: $<HTMLDetailsElement>("columns-panel"),
  columnsSummary: $("columns-summary"),
  columnFilter: $<HTMLInputElement>("column-filter"),
  tickAll: $<HTMLButtonElement>("tick-all"),
  untickAll: $<HTMLButtonElement>("untick-all"),
  untickSuggested: $<HTMLButtonElement>("untick-suggested"),
  suggested: $("suggested"),
  columnGroups: $("column-groups"),
  how: $("how"),
  structure: $("structure"),
  otherFile: $<HTMLInputElement>("other-file"),
  addOther: $<HTMLButtonElement>("add-other"),
  joined: $("joined"),
  removeOther: $<HTMLButtonElement>("remove-other"),
  textRelations: $<HTMLDetailsElement>("text-relations"),
  textCount: $("text-count"),
  textList: $("text-list"),
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
  /** Formulas found but not reported, best first. */
  nearMisses: NearMiss[];
  /** The options of the last scan, to search part of the rows again. */
  options?: ReturnType<typeof scanOptions>;
  suspects: Suspect[];
  /** The sheet or table being shown, when the file has several. */
  sheet?: string;
  /** A second file whose rows are matched with this one's. */
  other?: { name: string; bytes: ArrayBuffer };
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

/** Loads one sheet (or SQLite table) of a file into the engine. */
/** A file as the engine reads it: columnar formats are read in the page and handed over as JSON. */
async function engineFile(name: string, bytes: ArrayBuffer, sheet?: string) {
  const format = columnarFormat(name, bytes);
  if (!format) return { name, bytes, sheet, sheets: null as string[] | null };
  // Read in the page (the reader is loaded on demand), then handed over as JSON.
  const table = await readColumnar(format, bytes, sheet);
  return { name: `${name}.json`, bytes: table.json, sheet: undefined, sheets: table.sheets };
}

async function loadSheet(
  name: string,
  bytes: ArrayBuffer,
  sheet?: string,
  other?: { name: string; bytes: ArrayBuffer },
): Promise<DatasetInfo> {
  const first = await engineFile(name, bytes, sheet);
  const second = other ? await engineFile(other.name, other.bytes) : undefined;
  const info = await engine.load(first.name, first.bytes, first.sheet, second && { name: second.name, bytes: second.bytes });
  return first.sheets ? { ...info, sheets: first.sheets } : info;
}

async function openFile(name: string, bytes: ArrayBuffer, sheet?: string, other?: { name: string; bytes: ArrayBuffer }) {
  showError(null);
  try {
    let info = await loadSheet(name, bytes, sheet, other);
    // Without a choice, skip sheets or tables with no numbers (a list of customers).
    if (info.columns.length === 0 && sheet === undefined) {
      for (const otherSheet of info.sheets.slice(1)) {
        const next = await loadSheet(name, bytes, otherSheet, other);
        if (next.columns.length > 0) {
          info = next;
          sheet = otherSheet;
          break;
        }
      }
    }
    if (info.columns.length === 0) throw new Error("No numeric columns found in this file.");
    const suggested = suggestions(info);
    const ignored =
      loadChoice(name, info) ?? new Set([...suggested].filter(([, s]) => s.untick).map(([n]) => n));
    state = { name, bytes, info, ignored, derived: [], nearMisses: [], suspects: [], sheet, other };
    setColumns([...info.header.map(headerName), ...info.columns.map((c) => c.name)]);
    renderData();
  } catch (e) {
    const what = other ? `${name} with ${other.name}` : name;
    showError(`Could not read ${what}: ${(e as Error).message}`);
  }
}

function renderData() {
  if (!state) return;
  const { info, name } = state;
  ui.start.hidden = true;
  ui.how.hidden = true;
  ui.results.hidden = true;
  ui.suspicious.hidden = true;
  ui.data.hidden = false;
  ui.fileName.textContent = name;
  const texts = info.skipped.length + info.categories.length;
  const text = texts ? `, ${texts} text` : "";
  ui.fileSummary.textContent = `${info.rows} rows, ${info.columns.length} numeric columns${text}`;

  ui.sheetWrap.hidden = info.sheets.length < 2;
  ui.sheetLabel.textContent = columnarFormat(name, state.bytes) === "sqlite" ? "Table" : "Sheet";
  ui.sheet.replaceChildren(...info.sheets.map((s) => el("option", { value: s }, s)));
  if (state.sheet && info.sheets.includes(state.sheet)) ui.sheet.value = state.sheet;

  renderStructure();
  renderPreview(false);
  renderColumnPanel();
  renderColumnChoices();
  ui.ownResult.replaceChildren();
  showOwnError(null);
}

/** What each kind of column is for, in a few words. */
const ROLE_NAMES: Record<string, string> = {
  constant: "constant",
  flag: "yes / no",
  "group key": "groups rows",
  order: "order",
  regime: "type",
  code: "code",
  identifier: "ID",
  measurement: "measurement",
};

/** The role of each column, by name. */
function roles(): Map<string, string> {
  return new Map((state?.info.structure.roles ?? []).map((r) => [r.column, ROLE_NAMES[r.role] ?? r.role]));
}

/** How the table is laid out, and the second file when one is matched. */
function renderStructure() {
  if (!state) return;
  const { structure, joined } = state.info;
  const types = structure.roles.filter((r) => r.role === "regime").map((r) => r.column);
  const chip = (label: string, value: string, tip: string) =>
    el("li", { title: tip }, el("span", { class: "muted" }, `${label} `), el("b", {}, value));
  const items: HTMLElement[] = [];
  if (structure.groups.length)
    items.push(chip("Rows grouped by", structure.groups.join(", "), "Each group is a separate sequence, such as one track of one line."));
  if (structure.order)
    items.push(chip("In order of", structure.order, "Formulas between consecutive rows are looked for along this column."));
  if (types.length)
    items.push(chip("Types", types.join(", "), "A formula may change with the value of these columns, such as a curve type."));
  ui.structure.replaceChildren(...items);
  ui.structure.hidden = items.length === 0;
  ui.joined.textContent = !joined
    ? "Find relations between two files that share columns such as a line and a chainage, or that both give coordinates."
    : joined.median_distance != null
      ? `Matched ${joined.matched} of ${joined.rows} rows to the nearest point of the other file on ` +
        `${joined.keys.join(", ")}, median distance ${fmt(joined.median_distance)}.`
      : `Matched ${joined.matched} of ${joined.rows} rows with ${joined.file} on ${joined.keys.join(", ")}.` +
        (joined.repeated ? ` ${joined.repeated} rows of ${joined.file} repeat an earlier one and were left out.` : "");
  ui.addOther.textContent = joined ? "⇄ Match with another file" : "⇄ Match with a second file";
  ui.removeOther.hidden = !joined;
}

/** Columns of the preview shown before "Show all columns" on wide files. */
const PREVIEW_COLUMNS = 30;

/** The first rows of the file, with a checkbox per numeric column. */
function renderPreview(all: boolean) {
  if (!state) return;
  const { info } = state;
  const wide = info.header.length > PREVIEW_COLUMNS + 10;
  const shown = wide && !all ? info.header.slice(0, PREVIEW_COLUMNS) : info.header;
  ui.previewMore.hidden = !wide;
  ui.previewMore.textContent = all ? `Show the first ${PREVIEW_COLUMNS} columns` : `Show all ${info.header.length} columns`;
  ui.previewMore.onclick = () => renderPreview(!all);
  const numeric = new Set(info.columns.map((c) => c.name));
  const readAs = new Map(info.columns.map((c) => [c.name, c.read_as]));
  const role = roles();
  const head = el("tr", {}, el("th", { class: "text" }, "#"));
  for (const col of shown) {
    if (!numeric.has(col)) {
      const kind = info.categories.includes(col) ? `text, ${role.get(col) ?? "category"}` : "text, skipped";
      head.append(el("th", { class: "text colhead", style: `--col: ${columnColor(col)}` }, col, el("span", { class: "kind" }, kind)));
      continue;
    }
    const as = readAs.get(col);
    const r = role.get(col);
    const kind = as ?? (r && r !== "measurement" ? `number, ${r}` : "number");
    head.append(el("th", { class: "colhead", style: `--col: ${columnColor(col)}` }, el("label", {}, columnBox(col), col), el("span", { class: "kind" }, kind)));
  }
  const rows = info.preview.map((cells, i) =>
    el(
      "tr",
      { "data-row": String(i + 1) },
      el("td", { class: "rownum" }, String(i + 1)),
      ...shown.map((col) => el("td", numeric.has(col) ? {} : { class: "text" }, cells[info.header.indexOf(col)] ?? "")),
    ),
  );
  ui.preview.replaceChildren(el("thead", {}, head), el("tbody", {}, ...rows));
  for (const d of state.derived) highlight(d.exception_rows);
}

/** A checkbox that ticks or unticks one column wherever it is shown. */
function columnBox(col: string): HTMLInputElement {
  const box = el("input", { type: "checkbox", "data-column": col });
  box.checked = !state!.ignored.has(col);
  box.addEventListener("change", () => setIgnored([col], !box.checked));
  return box;
}

/** Ticks (`ignored` false) or unticks columns, everywhere, and remembers the choice for the file. */
function setIgnored(columns: string[], ignored: boolean) {
  if (!state) return;
  for (const c of columns) {
    if (ignored) state.ignored.add(c);
    else state.ignored.delete(c);
  }
  saveChoice(state.name, state.ignored);
  syncColumnBoxes();
  renderColumnChoices();
}

function syncColumnBoxes() {
  if (!state) return;
  for (const box of document.querySelectorAll<HTMLInputElement>("input[data-column]")) {
    box.checked = !state.ignored.has(box.dataset.column ?? "");
  }
  for (const box of ui.columnGroups.querySelectorAll<HTMLInputElement>("input[data-group]")) {
    const names = (box.dataset.group ?? "").split("\n");
    const ticked = names.filter((n) => !state!.ignored.has(n)).length;
    box.checked = ticked === names.length;
    box.indeterminate = ticked > 0 && ticked < names.length;
  }
  const n = state.info.columns.length;
  ui.columnsSummary.textContent = `(${n - state.ignored.size} of ${n})`;
}

/** The "Columns to scan" panel: filter, tick or untick all, suggestions, groups by prefix. */
function renderColumnPanel() {
  if (!state) return;
  const { info } = state;
  const names = info.columns.map((c) => c.name);
  const suggested = suggestions(info);
  ui.columnFilter.value = "";
  ui.columnsPanel.open = names.length > PREVIEW_COLUMNS + 10;
  // One line per reason, naming a few columns each.
  const byReason = new Map<string, string[]>();
  for (const [n, s] of suggested) byReason.set(s.why, [...(byReason.get(s.why) ?? []), n]);
  ui.suggested.replaceChildren(
    ...(suggested.size ? [`Suggested to leave out (${suggested.size}, marked ⚑):`] : []),
    ...[...byReason].map(([why, cols]) => {
      const shown = cols.slice(0, 4).flatMap((n, i) => [i ? ", " : "", el("code", {}, n)]);
      const more = cols.length > 4 ? ` and ${cols.length - 4} more` : "";
      return el("span", { class: "reason" }, `${cols.length} with ${why}: `, ...shown, more, ".");
    }),
  );
  ui.untickSuggested.hidden = suggested.size === 0;
  ui.untickSuggested.onclick = () => setIgnored([...suggested.keys()], true);
  ui.columnGroups.replaceChildren(
    ...groups(names).map((g) => {
      const all = el("input", { type: "checkbox", "data-group": g.columns.join("\n") });
      all.addEventListener("change", () => setIgnored(g.columns, !all.checked));
      const legend = g.prefix
        ? el("legend", {}, el("label", {}, all, ` ${g.prefix}_… `, el("span", { class: "muted" }, `(${g.columns.length})`)))
        : el("legend", {}, el("label", {}, all, " Other columns ", el("span", { class: "muted" }, `(${g.columns.length})`)));
      return el(
        "fieldset",
        { class: "column-group" },
        legend,
        ...g.columns.map((c) =>
          el("label", { "data-name": c.toLowerCase() }, columnBox(c), " ", c, suggested.has(c) ? el("span", { class: "muted" }, " ⚑") : ""),
        ),
      );
    }),
  );
  syncColumnBoxes();
}

/** Columns that match the filter box. */
function filteredColumns(): string[] {
  const q = ui.columnFilter.value.trim().toLowerCase();
  return state ? state.info.columns.map((c) => c.name).filter((n) => n.toLowerCase().includes(q)) : [];
}

function filterColumnPanel() {
  const q = ui.columnFilter.value.trim().toLowerCase();
  for (const label of ui.columnGroups.querySelectorAll<HTMLElement>("label[data-name]")) {
    label.hidden = !(label.dataset.name ?? "").includes(q);
  }
  for (const group of ui.columnGroups.querySelectorAll<HTMLElement>("fieldset")) {
    group.hidden = !group.querySelector("label[data-name]:not([hidden])");
  }
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

/** The options of a scan with the current settings. */
function scanOptions(ignore: string[]) {
  return { ignore, ...searchOptions(settings, state?.info.rows ?? 0) };
}

/** One line on what the search for a column is doing, from its events. */
function describeEvent(e: SearchEvent): (Node | string)[] {
  const percent = (matched: number, checked: number) => `${Math.floor((100 * matched) / Math.max(1, checked))}%`;
  switch (e.kind) {
    case "started":
      return [`searching ${e.rows.toLocaleString()} rows with ${e.inputs.join(", ") || "no inputs"}`];
    case "best": {
      const sample =
        e.rows_total && e.checked < e.rows_total
          ? [" ", el("span", { class: "provisional" }, `provisional, ${e.checked.toLocaleString()} of ${e.rows_total.toLocaleString()} rows checked`)]
          : [];
      return [
        "best so far ",
        el("code", {}, ...painted(e.formula)),
        ` (${percent(e.matched, e.checked)} of rows)`,
        ...sample,
      ];
    }
    case "confirming":
      return [
        "found ",
        el("code", {}, ...painted(e.formula)),
        ` on ${e.sample.toLocaleString()} sample rows, checking all ${e.rows.toLocaleString()} rows`,
      ];
    case "second_formula":
      return [`looking for a second formula on the ${e.rows.toLocaleString()} rows that break it`];
    case "done":
      return [e.found ? "found" : e.near_misses ? "no formula, kept what came close" : "no formula"];
  }
}

async function scanAll() {
  if (!state) return;
  const usable = state.info.columns.map((c) => c.name).filter((n) => !state!.ignored.has(n));
  const columns = usable.includes(ui.only.value) ? [ui.only.value] : usable;
  const ignore = [...state.ignored];
  const options = scanOptions(ignore);
  state.options = options;
  state.nearMisses = [];
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
  // Columns are handed out from a queue to one task per worker, so a slow
  // column does not hold up the others. On a large file there are two
  // passes: a quick look at every column on a sample, whose formulas are
  // shown at once as provisional, then each of them checked on all rows.
  const sample = (options as ExplainOptions).sample ?? 0;
  const found = new Map<string, Derived>();
  const provisional = new Map<string, Derived>();
  const near = new Map<string, NearMiss[]>();
  const done = new Set<string>();
  const dropped = new Set<string>();
  const running = new Set<string>();
  // What each running column's search is doing, from its events, redrawn
  // at most once per frame.
  const doing = new Map<string, SearchEvent>();
  let drawing = false;
  const drawLive = () => {
    if (drawing) return;
    drawing = true;
    requestAnimationFrame(() => {
      drawing = false;
      // Every column, in order: found, nothing, being searched (with what
      // the search is doing) or still to do.
      ui.progressLive.replaceChildren(
        ...columns.map((c) => {
          const state = (cls: string, mark: string, ...what: (Node | string)[]) =>
            el("li", { class: cls }, el("span", { class: "state", "aria-hidden": "true" }, mark), columnName(c), el("span", { class: "what" }, ...what));
          const formula = (d: Derived) => (d.regime ? ["depends on ", columnName(d.regime.column, "span")] : [el("code", {}, ...painted(d.formula))]);
          if (running.has(c)) {
            const e = doing.get(c);
            const li = state("running", "", ...(e ? describeEvent(e) : [provisional.has(c) ? "checking on all rows" : "waiting for a worker"]));
            return li;
          }
          const d = found.get(c);
          if (d) return state("found", "✓", ...formula(d));
          if (dropped.has(c)) return state("none", "–", "found on the sample, not on all rows");
          const p = provisional.get(c);
          if (p) return state("provisional", "≈", ...formula(p), el("span", { class: "muted" }, ` · on ${sample.toLocaleString()} rows, to be checked`));
          if (done.has(c)) return state("none", "–", "no formula");
          return el("li", { class: "todo" }, el("span", { class: "state", "aria-hidden": "true" }), columnName(c));
        }),
      );
    });
  };
  ui.progressLive.replaceChildren();
  const started = performance.now();
  // Two queues share the workers: quick looks on the sample first, and each
  // formula a look found is checked on all rows as soon as a worker is
  // free, without waiting for the other looks. A check runs on the worker
  // that found the formula when that one is free (it skips searching the
  // sample again), else on any other.
  const looks: string[] = sample ? [...columns] : [];
  const checks: string[] = sample ? [] : [...columns];
  let looked = 0;
  let checked = 0;
  const progress = () => {
    const names = [...running].slice(0, 3).join(", ") + (running.size > 3 ? "…" : "");
    const rows = state!.info.rows.toLocaleString();
    const total = sample ? columns.length + provisional.size : columns.length;
    const left = timeLeft(performance.now() - started, looked + checked, total);
    const what = sample
      ? `Quick look on ${sample.toLocaleString()} of ${rows} rows: ${looked} of ${columns.length} columns` +
        (provisional.size ? ` · checked on all rows: ${checked} of ${provisional.size} formulas` : "")
      : `Checked ${checked} of ${columns.length} columns`;
    ui.progressText.textContent = `${what}${left ? ` · ${left}` : ""}${names ? ` · now ${names}` : ""}…`;
    ui.meter.style.width = `${(100 * (looked + checked)) / Math.max(1, total)}%`;
  };
  const live = liveResults(columns);
  // The worker that took a quick look at each column, to check it there.
  const slots = new Map<string, number | undefined>();
  // In column order, whatever order they were found in.
  const inOrder = () => columns.filter((c) => found.has(c)).map((c) => found.get(c) as Derived);
  const nearInOrder = () =>
    columns.flatMap((c) => near.get(c) ?? []).sort((a, b) => b.matched_rows - a.matched_rows);
  const keepMisses = (column: string, misses: NearMiss[]) => {
    if (!misses.length) return;
    near.set(column, misses);
    renderNearMisses(nearInOrder());
  };
  const search = async (column: string, look: boolean) => {
    running.add(column);
    progress();
    drawLive();
    const onEvent = (e: SearchEvent) => {
      if (e.kind === "done") return;
      doing.set(column, e);
      drawLive();
    };
    const pinned = slots.get(column);
    const where = { slot: !look && engine.idle(pinned) ? pinned : undefined };
    const opts = look ? { ...options, sample_only: true } : options;
    const { found: d, near_misses } = await engine.explain(column, opts, onEvent, where);
    slots.set(column, where.slot);
    running.delete(column);
    doing.delete(column);
    if (stopped) return;
    if (look) {
      looked++;
      keepMisses(column, near_misses);
      if (d) {
        provisional.set(column, d);
        live.add({ ...d, provisional: true }, [...provisional.values()]);
        checks.push(column);
      } else {
        done.add(column);
      }
    } else {
      checked++;
      done.add(column);
      if (!sample) keepMisses(column, near_misses);
      if (d) {
        found.set(column, d);
        live.add(d, inOrder());
      } else if (sample) {
        dropped.add(column);
        live.remove(column);
      }
    }
    drawLive();
    progress();
  };
  // Hands out work until both queues are empty and nothing is running.
  const runAll = () =>
    new Promise<void>((resolve, reject) => {
      let active = 0;
      const pump = () => {
        while (!stopped && active < engine.workers && (looks.length || checks.length)) {
          // Quick looks first, so every column gets a first answer soon.
          const look = looks.length > 0;
          const column = (look ? looks.shift() : checks.shift()) as string;
          active++;
          search(column, look).then(
            () => {
              active--;
              pump();
            },
            reject,
          );
        }
        if (active === 0) resolve();
      };
      pump();
    });
  try {
    progress();
    await runAll();
    ui.meter.style.width = "100%";
    state.derived = inOrder();
    state.nearMisses = nearInOrder();
    if (state.derived.length > 1) {
      const reviewed = await engine.review(ignore, state.derived);
      state.derived = state.derived.map((d, i) => (reviewed[i] ? { ...d, exceptions: reviewed[i].exceptions } : d));
    }
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
      if (stopped) {
        state.derived = inOrder();
        state.nearMisses = nearInOrder();
      }
      const checked = columns.filter((c) => done.has(c));
      const skipped = columns.filter((c) => !done.has(c));
      renderResults(checked, stopped && { why: stopped, skipped });
      suspectsShown = SUSPECTS_PAGE;
      renderSuspects();
    }
  } catch (e) {
    showError(`Something went wrong: ${(e as Error).message}`);
  } finally {
    ui.scan.disabled = false;
    ui.progress.hidden = true;
    ui.progressLive.replaceChildren();
  }
}

/**
 * The formulas found but not reported, best first, each with a button that
 * searches the rows where it fails.
 */
function renderNearMisses(misses: NearMiss[]) {
  ui.almost.hidden = misses.length === 0;
  if (misses.length) ui.results.hidden = false;
  ui.almostCount.textContent = `(${misses.length})`;
  ui.almostList.replaceChildren(...misses.map(nearMissItem));
}

function nearMissItem(m: NearMiss): HTMLElement {
  const share = `${Math.floor(100 * m.match_ratio)}%`;
  const failing = m.missed_count;
  const item = el(
    "li",
    { "data-column": m.column },
    el("div", { class: "formula" }, columnName(m.column), " = ", ...painted(m.formula)),
    el(
      "div",
      { class: "meta" },
      el("span", { class: "warn" }, `${m.matched_rows.toLocaleString()} of ${m.checked_rows.toLocaleString()} rows (${share})`),
      usesNode(m.inputs),
    ),
    el("p", { class: "why muted" }, `Not reported: ${m.reason_text}.`),
  );
  if (m.holds_when) {
    item.append(el("p", { class: "why" }, "Holds where ", el("code", {}, m.holds_when.rule), ` (${Math.round(100 * m.holds_when.accuracy)}% of rows agree).`));
  }
  if (m.regime) item.append(el("p", { class: "why" }, `Searched within ${m.regime}.`));
  if (failing > 0) {
    const out = el("div", { class: "rerun" });
    const button = el("button", { type: "button" }, `Search the ${failing.toLocaleString()} rows where it fails`);
    button.addEventListener("click", () => searchFailingRows(m, button, out));
    item.append(button, out);
  }
  return item;
}

/** Searches the column again on the rows where a near miss fails. */
async function searchFailingRows(m: NearMiss, button: HTMLButtonElement, out: HTMLElement) {
  if (!state) return;
  button.disabled = true;
  const options = { ...(state.options ?? scanOptions([...state.ignored])), failing: m.formula };
  const line = el("p", { class: "muted" }, "Searching…");
  out.replaceChildren(line);
  try {
    const r = await engine.explain(m.column, options, (e) => {
      if (e.kind !== "done") line.replaceChildren("Searching: ", ...describeEvent(e));
    });
    const parts: HTMLElement[] = [];
    if (r.found) parts.push(derivedCard(r.found));
    else parts.push(el("p", { class: "muted" }, `No formula holds on ${m.missed_count === 1 ? "that row" : `most of those ${m.missed_count.toLocaleString()} rows`}.`));
    if (r.near_misses.length) {
      parts.push(el("p", { class: "why muted" }, "Came close on those rows:"));
      parts.push(el("ul", { class: "almost" }, ...r.near_misses.map(nearMissItem)));
    }
    out.replaceChildren(...parts);
  } catch (e) {
    out.replaceChildren(el("p", { class: "error" }, `Something went wrong: ${(e as Error).message}`));
  } finally {
    button.disabled = false;
  }
}

/**
 * Shows each formula as soon as its column is checked, in column order. The
 * dependency diagram, the copies' review and the suspicious rows wait for the
 * end of the scan, when `renderResults` redraws everything.
 */
function liveResults(columns: string[]) {
  const position = new Map(columns.map((c, i) => [c, i]));
  ui.graph.hidden = true;
  ui.graphWrap.replaceChildren();
  ui.copies.hidden = true;
  ui.almost.hidden = true;
  ui.almostList.replaceChildren();
  ui.cards.replaceChildren();
  ui.independent.textContent = "";
  return {
    add(d: Derived, all: Derived[]) {
      ui.results.hidden = false;
      ui.independent.textContent = "Still searching: more formulas may appear below.";
      if (d.copy_of) return renderCopies(all.filter((x) => x.copy_of));
      const card = derivedCard(d);
      const old = ui.cards.querySelector<HTMLElement>(`article[data-column="${CSS.escape(d.column)}"]`);
      if (old) {
        old.replaceWith(card);
        if (!d.provisional) card.classList.add("flash");
        return;
      }
      const after = [...ui.cards.querySelectorAll<HTMLElement>("article[data-column]")].find(
        (c) => (position.get(c.dataset.column ?? "") ?? Infinity) > (position.get(d.column) ?? Infinity),
      );
      ui.cards.insertBefore(card, after ?? null);
    },
    /** Takes away a provisional formula that did not hold on all rows. */
    remove(column: string) {
      ui.cards.querySelector(`article[data-column="${CSS.escape(column)}"]`)?.remove();
    },
  };
}

/** "about 3 min left", from the time per column so far; empty until there is enough to go on. */
function timeLeft(elapsedMs: number, done: number, total: number): string {
  if (done < 2 || done >= total || elapsedMs < 3000) return "";
  const seconds = ((elapsedMs / done) * (total - done)) / 1000;
  if (seconds < 60) return `about ${Math.max(5, Math.round(seconds / 5) * 5)} s left`;
  if (seconds < 3600) return `about ${Math.round(seconds / 60)} min left`;
  return `about ${(seconds / 3600).toFixed(1)} h left`;
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
  const found = state.derived.filter((d) => !d.copy_of).length;
  const copies = state.derived.length - found;
  const parts: (Node | string)[] = [
    el("span", {}, found === 1 ? "1 formula found" : `${found} formulas found`),
  ];
  if (copies) parts.push(el("span", { class: "muted" }, copies === 1 ? "1 copy of another column" : `${copies} copies of other columns`));
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
  // A long list is shown a page at a time; the CSV export has every row.
  const shown = suspects.slice(0, suspectsShown);
  const body = shown.map((s) => {
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
  const rest = n - shown.length;
  ui.suspiciousMore.hidden = rest === 0;
  ui.suspiciousMore.textContent = `Show ${Math.min(rest, SUSPECTS_PAGE).toLocaleString()} more of ${rest.toLocaleString()} (all are in the CSV export)`;
}

/** Suspicious rows shown at first, and added by each "Show more". */
const SUSPECTS_PAGE = 100;
let suspectsShown = SUSPECTS_PAGE;

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

/** Every column name of the file, to colour them in formulas. */
function allNames(): string[] {
  if (!state) return [];
  return [...state.info.header.map(headerName), ...state.info.columns.map((c) => c.name)];
}

/** Formula text with its column names in their colours. */
function painted(text: string): (Node | string)[] {
  return colorize(text, allNames());
}

/** "uses a, b": the inputs of a formula, each in its colour. */
function usesNode(inputs: string[]): HTMLElement {
  const names = inputs.flatMap((n, i) => (i ? [", ", columnName(n, "span")] : [columnName(n, "span")]));
  return el("span", { class: "muted" }, "uses ", ...names);
}

function formulaNode(d: Derived): HTMLElement {
  if (d.regime) {
    return el("div", { class: "formula" }, columnName(d.column), el("span", { class: "muted" }, ` depends on `), columnName(d.regime.column));
  }
  return el("div", { class: "formula" }, columnName(d.column), " = ", ...painted(d.formula));
}

/** Why a formula is partial, in words: how many rows break it and what that may mean. */
function partialNote(d: Derived, exceptions: number): HTMLElement {
  const share = (100 * exceptions) / Math.max(1, d.checked_rows);
  const pct = share < 0.1 ? "less than 0.1%" : `${share < 10 ? share.toFixed(1) : Math.round(share)}%`;
  const rows = exceptions === 1 ? "1 row does" : `${exceptions.toLocaleString()} rows (${pct}) do`;
  const why = fewExceptions(d)
    ? "So few that they are probably typing or copy mistakes: each one is listed below, and under Suspicious rows with the cell to check."
    : d.regime
      ? "They are usually where the type changes, or stretches where the value was set by hand."
      : "They may be a separate group (another rate, product or period) rather than mistakes.";
  return el("p", { class: "partial-note" }, el("b", {}, "Partial: "), `${rows} not follow this formula. ${why}`);
}

/** Badges that say what kind of formula it is. */
function kindBadges(d: Derived): HTMLElement[] {
  const out: HTMLElement[] = [];
  if (d.regime) out.push(el("span", { class: "badge kind-badge", title: "A formula for each value of a text column" }, `per ${d.regime.column}`));
  const steps = d.between_rows || d.regime?.pieces.some((p) => p.formula.includes("slope of"));
  if (steps) out.push(el("span", { class: "badge kind-badge", title: "Links each row to the previous one, in the order of the table" }, "between rows"));
  if (d.within !== undefined && d.within !== null)
    out.push(el("span", { class: "badge approx-badge", title: "Holds only to within this error" }, `≈ approximate, within ${Number(d.within.toPrecision(2))}`));
  return out;
}

/** One row per value of the regime column: its formula and how many rows follow it. */
function regimeTable(d: Derived): HTMLElement {
  const regime = d.regime as NonNullable<Derived["regime"]>;
  const rows = regime.pieces.map((p) => {
    const ok = p.matched_rows === p.rows;
    const formula = p.formula.replace(/^about /, "≈ ");
    const runs = p.per_run && p.runs ? el("span", { class: "muted" }, ` · ${p.runs} runs`) : "";
    return el(
      "tr",
      {},
      el("th", { scope: "row" }, p.value),
      el("td", { class: "piece" }, el("code", {}, ...painted(formula)), runs),
      el("td", { class: ok ? "ok num" : "warn num" }, ok ? `✓ ${p.rows}` : `${p.matched_rows} of ${p.rows}`),
    );
  });
  return el(
    "table",
    { class: "pieces" },
    el("thead", {}, el("tr", {}, el("th", {}, columnName(regime.column, "span")), el("th", {}, "formula"), el("th", { class: "num" }, "rows"))),
    el("tbody", {}, ...rows),
  );
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
        : el("span", { class: "warn", title: "Partial: the formula holds on most rows, not all" }, `Partial · ${d.matched_rows} of ${d.checked_rows} rows`);
  const state = d.provisional ? "provisional" : exceptions === 0 ? "exact" : d.other && !wrong ? "exact" : "partial";
  if (d.provisional) {
    // A quick first look: shown at once, checked on all rows next.
    return el(
      "article",
      { class: "card provisional", "data-column": d.column },
      formulaNode(d),
      el(
        "div",
        { class: "meta" },
        el("span", { class: "badge preview-badge" }, "Preview"),
        el("span", {}, `${d.matched_rows.toLocaleString()} of ${d.checked_rows.toLocaleString()} sample rows`),
        ...kindBadges(d),
        usesNode(d.inputs),
      ),
      ...(d.regime ? [regimeTable(d)] : []),
      el("p", { class: "note" }, el("span", { class: "spinner", "aria-hidden": "true" }), " Found on a sample of the rows. Now checking it on every row…"),
    );
  }
  const card = el(
    "article",
    { class: `card ${state}${d.within != null ? " approx" : ""}`, "data-column": d.column },
    formulaNode(d),
    el("div", { class: "meta" }, status, ...kindBadges(d), usesNode(d.inputs)),
  );
  if (d.regime) card.append(regimeTable(d));
  if (exceptions > 0 && !d.other) card.append(partialNote(d, exceptions));
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
        el("span", { class: "formula-inline" }, columnName(d.column), " = ", ...painted(o.formula)),
        el("span", { class: "muted" }, o.split ? ` for ${n}, where ${o.split.rule}` : ` for ${n}`),
      ),
    );
    if (!o.split) {
      card.append(el("p", { class: "note" }, "No column tells these rows apart; a text column such as a product or region may."));
    }
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
    if (d.exceptions.length === 0) {
      // A formula per type or between rows has no single expected value per row.
      const list = d.exception_rows.slice(0, 50).join(", ") + (exceptions > 50 ? ", …" : "");
      card.append(
        el(
          "details",
          {},
          el("summary", {}, `${exceptions === 1 ? "1 row breaks" : `${exceptions} rows break`} the rule`),
          el("p", { class: "muted" }, `Rows ${list}.`),
        ),
      );
      card.append(chartToggle(d));
      return card;
    }
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
  const main = state.derived.filter((d) => !d.copy_of);
  const copies = state.derived.filter((d) => d.copy_of);
  const cards = main.map(derivedCard);
  for (const d of state.derived) highlight(d.exception_rows);
  ui.cards.replaceChildren(
    ...(cards.length || copies.length ? cards : [el("p", {}, "No column could be computed from the others.")]),
  );
  renderCopies(copies);
  renderTextRelations();
  renderNearMisses(state.nearMisses);
  renderGraph(main);
  const found = new Set(state.derived.map((d) => d.column));
  const rest = columns.filter((c) => !found.has(c));
  const notes = rest.length ? [`No formula found for: ${rest.join(", ")}.`] : [];
  if (stopped) {
    const why = stopped.why === "time" ? "The time limit was reached" : "The search was stopped";
    notes.push(`${why} before checking: ${stopped.skipped.join(", ")}.`);
  }
  ui.independent.textContent = notes.join(" ");
}

/** Relations between text columns, as found when the file was read. */
function renderTextRelations() {
  const rels = state?.info.text_relations ?? [];
  ui.textRelations.hidden = rels.length === 0;
  ui.textCount.textContent = `(${rels.length})`;
  const text = (r: TextRelation) => {
    if (r.formula.startsWith("set by ")) return `each ${r.inputs[0]} has one ${r.column}`;
    if (r.formula.startsWith("one to one with ")) return `${r.column} and ${r.inputs.join(", ")} go one to one`;
    return `${r.column} = ${r.formula}`;
  };
  ui.textList.replaceChildren(
    ...rels.map((r) =>
      el(
        "li",
        {},
        el("code", {}, ...painted(text(r))),
        r.matched_rows < r.checked_rows ? el("span", { class: "muted" }, ` (${r.matched_rows} of ${r.checked_rows} rows)`) : "",
      ),
    ),
  );
}

/** One line per copied column: `a = b = c`, then `d = a + 1, e = a + 2`. */
function renderCopies(copies: Derived[]) {
  ui.copies.hidden = copies.length === 0;
  ui.copiesCount.textContent = `(${copies.length})`;
  const originals = [...new Set(copies.map((d) => d.copy_of as string))];
  const differ = (d: Derived) => (d.exception_rows.length ? ` (${d.matched_rows}/${d.checked_rows} rows)` : "");
  ui.copiesList.replaceChildren(
    ...originals.map((original) => {
      const group = copies.filter((d) => d.copy_of === original);
      const same = group.filter((d) => d.formula === original);
      const parts = same.length ? [[original, ...same.map((d) => d.column + differ(d))].join(" = ")] : [];
      for (const d of group.filter((d) => d.formula !== original)) parts.push(`${d.column} = ${d.formula}${differ(d)}`);
      return el("li", {}, el("code", {}, parts.join(", ")));
    }),
  );
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
    near_misses: state.nearMisses,
  };
  const blob = new Blob([JSON.stringify(report, null, 2)], { type: "application/json" });
  const a = el("a", { href: URL.createObjectURL(blob), download: `${state.name.replace(/\.[^.]+$/, "")}.regrex.json` });
  a.click();
  URL.revokeObjectURL(a.href);
}

/** Whether a file name has an extension the page cannot read (the list is the file input's `accept`). */
function unsupportedExtension(name: string): string | null {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return null; // No extension: read it as CSV.
  const ext = name.slice(dot).toLowerCase();
  return ui.file.accept.split(",").includes(ext) ? null : ext;
}

async function readFile(file: File) {
  const ext = unsupportedExtension(file.name);
  if (ext) {
    const formats = [...document.querySelectorAll("#formats b")].map((b) => b.textContent).join(", ");
    showError(`Regrex cannot read ${ext} files. Supported formats: ${formats}.`);
    return;
  }
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
  if (state) void openFile(state.name, state.bytes, ui.sheet.value, state.other);
});
ui.scan.addEventListener("click", () => void scanAll());
ui.columnFilter.addEventListener("input", filterColumnPanel);
// With a filter, only the columns that match it.
ui.tickAll.addEventListener("click", () => setIgnored(filteredColumns(), false));
ui.untickAll.addEventListener("click", () => setIgnored(filteredColumns(), true));
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

ui.addOther.addEventListener("click", () => ui.otherFile.click());
ui.otherFile.addEventListener("change", async () => {
  const file = ui.otherFile.files?.[0];
  ui.otherFile.value = "";
  if (!file || !state) return;
  await openFile(state.name, state.bytes, state.sheet, { name: file.name, bytes: await file.arrayBuffer() });
});
ui.removeOther.addEventListener("click", () => {
  if (state) void openFile(state.name, state.bytes, state.sheet);
});

ui.suspiciousMore.addEventListener("click", () => {
  suspectsShown += SUSPECTS_PAGE;
  renderSuspects();
});
