// Shapes of the JSON exchanged with the Rust engine (see crates/regrex-wasm).

export interface ColumnInfo {
  name: string;
  decimals: number | null;
  missing: number;
  /** How the cells were read when they are not plain numbers. */
  read_as: string | null;
  /** Different values, counted up to 1000. */
  distinct: number;
  /** Whether every value is larger than the one before, as in an ID. */
  increasing: boolean;
}

export interface DatasetInfo {
  rows: number;
  header: string[];
  preview: string[][];
  sheets: string[];
  columns: ColumnInfo[];
  skipped: string[];
  /** Text columns read as categories: they decide which formula applies. */
  categories: string[];
  /** How the table is laid out and what each column is for. */
  structure: Structure;
  /** Relations between text columns, such as `key = line + "|" + track`. */
  text_relations: TextRelation[];
  /** How the rows of a second file were matched, when one was added. */
  joined: Joined | null;
}

export interface TextRelation {
  column: string;
  /** `line + "|" + track`, `one to one with name` or `set by code`. */
  formula: string;
  inputs: string[];
  matched_rows: number;
  checked_rows: number;
}

export interface Joined {
  file: string;
  keys: string[];
  matched: number;
  rows: number;
  repeated: number;
}

/** A formula per value of a text column, such as a curve type. */
export interface Regime {
  column: string;
  pieces: {
    value: string;
    formula: string;
    per_run: boolean;
    rows: number;
    matched_rows: number;
    runs?: number;
    within?: number;
  }[];
}

export interface Structure {
  groups: string[];
  order: string | null;
  roles: { column: string; role: string; category: boolean; distinct: number; mean_run?: number }[];
}

export interface ExceptionDetail {
  row: number;
  actual: number;
  expected: number;
  diagnosis: Diagnosis | null;
}

export interface Diagnosis {
  kind: string;
  description: string;
  formula: string | null;
}

export interface KnownQuantity {
  symbol: string;
  name: string;
  unit: string;
  column: string | null;
  value: number | null;
}

export interface Known {
  id: string;
  name: string;
  domain: string;
  description: string | null;
  expression: string;
  output: KnownQuantity;
  variables: KnownQuantity[];
  changed_constant: { id: string; library_value: number; value: number } | null;
  exact: boolean;
  source_title: string;
  source_url: string | null;
}

export interface LibraryQuantity {
  symbol: string;
  name: string;
  unit: string;
}

export interface LibraryFormula {
  id: string;
  name: string;
  domain: string;
  description: string | null;
  expression: string;
  output: LibraryQuantity;
  variables: LibraryQuantity[];
  tags: string[];
  source_title: string;
  source_url: string | null;
}

export interface Library {
  domains: { id: string; name: string; description: string }[];
  formulas: LibraryFormula[];
}

export interface OtherFormula {
  formula: string;
  rows: number[];
  split: { column: string; rule: string; accuracy: number } | null;
}

export interface Derived {
  column: string;
  formula: string;
  inputs: string[];
  complexity: number;
  matched_rows: number;
  checked_rows: number;
  match_ratio: number;
  exception_rows: number[];
  exceptions: ExceptionDetail[];
  known: Known | null;
  other: OtherFormula | null;
  /** The column this one is a copy of (same values, maybe shifted or scaled). */
  copy_of?: string | null;
  /** A formula per value of a text column. */
  regime?: Regime;
  /** Whether the formula links each row to the previous one. */
  between_rows?: boolean;
  /** For an approximate relation, its largest error. */
  within?: number;
}

/**
 * A formula the search found but did not report: it holds on too few rows,
 * or only where another column has some values. Where to look next.
 */
export interface NearMiss {
  column: string;
  formula: string;
  inputs: string[];
  complexity: number;
  matched_rows: number;
  checked_rows: number;
  match_ratio: number;
  /** `coverage`, `usual_value`, `no_split_rule` or `other_values`. */
  reason: string;
  reason_text: string;
  /** A rule on another column that picks out the rows where it holds. */
  holds_when: { column: string; rule: string; accuracy: number } | null;
  /** The regime (value of a regime column) it was searched within, if any. */
  regime: string | null;
  missed_count: number;
  /** The first rows (numbered from 1) where it does not hold. */
  missed_rows: number[];
}

/** What the search for one column is doing, sent while it runs. */
export type SearchEvent =
  | { kind: "started"; column: string; inputs: string[]; rows: number }
  | {
      kind: "best";
      column: string;
      formula: string;
      matched: number;
      checked: number;
      /** Rows of the table, when the search only checked a sample of them. */
      rows_total?: number;
    }
  | { kind: "confirming"; column: string; formula: string; sample: number; rows: number }
  | { kind: "second_formula"; column: string; rows: number }
  | { kind: "done"; column: string; found: boolean; near_misses: number };

/** The answer to an `explain` request. */
export interface Explained {
  found: Derived | null;
  near_misses: NearMiss[];
}

export interface Candidate {
  formula: string;
  complexity: number;
  mse: number;
  r2: number;
  max_abs_error: number;
  exact: boolean;
  known: Known | null;
}

export interface FitReport {
  schema_version: number;
  target: string;
  inputs: string[];
  rows: number;
  candidates: Candidate[];
}

export interface ExplainOptions {
  ignore: string[];
  max_size?: number;
  max_params?: number;
  small_samples?: number;
  operators?: string[];
  min_match?: number;
  rel_tolerance?: number;
  max_inputs?: number;
  deepen_to?: number;
  library_only?: boolean;
  domain?: string;
  /** Only search these rows (numbered from 1). */
  rows?: number[];
}

/** A column's values and a formula's, for every row (null when missing). */
export interface Values {
  actual: (number | null)[];
  predicted: (number | null)[];
}

/** A domain of the built-in formula library. */
export interface Domain {
  id: string;
  name: string;
  description: string;
}

export type Request =
  | { kind: "load"; name: string; bytes: ArrayBuffer; sheet?: string; with?: { name: string; bytes: ArrayBuffer } }
  | { kind: "explain"; column: string; options: ExplainOptions }
  | { kind: "formula"; column: string; text: string }
  | { kind: "fit"; target: string; options: { inputs: string[] } }
  | { kind: "rows"; rows: number[] }
  | { kind: "values"; column: string; formula: string }
  | { kind: "domains" }
  | { kind: "review"; options: { ignore: string[]; derived?: Derived[] } };

export type Response =
  | { id: number; ok: true; value: unknown }
  | { id: number; ok: false; error: string }
  /** Sent while a request runs, before its answer. */
  | { id: number; event: SearchEvent };
