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
  | { kind: "load"; name: string; bytes: ArrayBuffer; sheet?: string }
  | { kind: "explain"; column: string; options: ExplainOptions }
  | { kind: "formula"; column: string; text: string }
  | { kind: "fit"; target: string; options: { inputs: string[] } }
  | { kind: "rows"; rows: number[] }
  | { kind: "values"; column: string; formula: string }
  | { kind: "domains" }
  | { kind: "review"; options: { ignore: string[]; derived?: Derived[] } };

export type Response =
  | { id: number; ok: true; value: unknown }
  | { id: number; ok: false; error: string };
