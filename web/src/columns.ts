// Choosing which columns of a wide file to scan: suggestions, groups by
// prefix and the choice remembered per file.

import type { DatasetInfo } from "./types";

/** Elasticsearch and Kibana metadata, and the text copies Kibana adds. */
const META = /^_(id|index|score|type|source|version|seq_no|primary_term|routing)$|\.keyword$/;

/**
 * Columns that are better left out of a scan, with why. Their checkboxes
 * start unticked when `untick` is true for the reason.
 */
export function suggestions(info: DatasetInfo): Map<string, { why: string; untick: boolean }> {
  const out = new Map<string, { why: string; untick: boolean }>();
  for (const c of info.columns) {
    const present = info.rows - c.missing;
    if (META.test(c.name)) out.set(c.name, { why: "search metadata names", untick: true });
    else if (c.distinct <= 1) out.set(c.name, { why: "the same value in every row", untick: true });
    else if (looksLikeId(c.name, info)) out.set(c.name, { why: "an ID-like name", untick: true });
    else if (c.increasing && c.decimals === 0 && c.distinct === present && present > 2)
      out.set(c.name, { why: "a larger whole number in each row, like an ID", untick: false });
  }
  return out;
}

/** Columns that look like identifiers: whole numbers, all different, named like an ID. */
export function looksLikeId(name: string, info: DatasetInfo): boolean {
  return /(^id$|_id$|^id_|number|^no\.?$|invoice|code|ref)/i.test(name) && info.columns.some((c) => c.name === name && c.decimals === 0);
}

/**
 * Columns grouped by the part of their name before the first `_` or `.`
 * (`coche0_temperatura` → `coche0`), when several share it. The rest go in
 * one group named "".
 */
export function groups(names: string[]): { prefix: string; columns: string[] }[] {
  const prefixOf = (n: string) => {
    const m = /^([^_.]+)[_.]/.exec(n);
    return m ? m[1] : "";
  };
  const count = new Map<string, number>();
  for (const n of names) count.set(prefixOf(n), (count.get(prefixOf(n)) ?? 0) + 1);
  const out = new Map<string, string[]>();
  for (const n of names) {
    const p = prefixOf(n);
    const key = p && (count.get(p) ?? 0) >= 2 ? p : "";
    if (!out.has(key)) out.set(key, []);
    out.get(key)!.push(n);
  }
  // Named groups first, in the order they appear; the rest at the end.
  return [...out.entries()]
    .sort((a, b) => Number(a[0] === "") - Number(b[0] === ""))
    .map(([prefix, columns]) => ({ prefix, columns }));
}

const key = (file: string) => `regrex.columns.${file}`;

/** The columns left out the last time this file was open, if any. */
export function loadChoice(file: string, info: DatasetInfo): Set<string> | null {
  try {
    const saved = JSON.parse(localStorage.getItem(key(file)) ?? "null") as unknown;
    if (!Array.isArray(saved)) return null;
    const names = new Set(info.columns.map((c) => c.name));
    return new Set(saved.filter((n): n is string => typeof n === "string" && names.has(n)));
  } catch {
    return null;
  }
}

export function saveChoice(file: string, ignored: Set<string>) {
  try {
    localStorage.setItem(key(file), JSON.stringify([...ignored]));
  } catch {
    // Storage may be full or blocked; the choice is then not remembered.
  }
}
