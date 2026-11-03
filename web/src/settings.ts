// Search settings: a profile (fast, normal, thorough), the operations
// formulas may use and a few advanced limits, each explained on the page.
// Saved in the browser so they survive a reload.

export type Profile = "fast" | "normal" | "thorough";

/** Where formulas come from: an open search, or only the built-in library. */
export type Source = "search" | "library";

export interface Settings {
  source: Source;
  /** With the library source, only this domain's formulas ("" for every domain). */
  domain: string;
  profile: Profile;
  /** Operations formulas may use, as accepted by the engine. */
  operators: string[];
  advanced: Advanced;
}

/** Limits under "Advanced". Each one is listed in {@link ADVANCED}. */
export interface Advanced {
  /** Percentage of rows a formula must match for a column to count as computed. */
  min_match: number;
  /** Relative tolerance for columns without a fixed number of decimals. */
  rel_tolerance: number;
  /** How many of the most related columns are tried as inputs. */
  max_inputs: number;
  /** Seconds before the search stops on its own, 0 for no limit. */
  time_limit: number;
}

/** How each advanced limit is shown and explained, with its allowed values. */
export const ADVANCED: {
  key: keyof Advanced;
  label: string;
  help: string;
  choices: { value: number; label: string }[];
}[] = [
  {
    key: "min_match",
    label: "Rows that must follow the formula",
    help:
      "A column counts as computed when its formula gives the right value in at least this share of rows. " +
      "Lower it to find rules with many exceptions, raise it to see only clean rules.",
    choices: [50, 70, 80, 90, 95, 99, 100].map((v) => ({ value: v, label: `${v} %` })),
  },
  {
    key: "rel_tolerance",
    label: "Tolerance",
    help:
      "Values always match within the precision written in the file: with 2 decimals, 12.10 matches 12.104. " +
      "On top of that, a value matches when it is within this percentage of what the formula gives. " +
      "Raise it for measured or noisy data.",
    choices: [
      { value: 1e-9, label: "exact" },
      { value: 1e-6, label: "0.0001 %" },
      { value: 1e-4, label: "0.01 %" },
      { value: 1e-3, label: "0.1 %" },
      { value: 1e-2, label: "1 %" },
      { value: 5e-2, label: "5 %" },
    ],
  },
  {
    key: "max_inputs",
    label: "Columns tried as inputs",
    help:
      "For each column, Regrex only combines the columns most related to it. " +
      "More columns can find formulas that mix many of them, but every extra column makes the search much slower. " +
      "Only known formulas always tries every column.",
    choices: [2, 3, 4, 5, 6, 8, 10].map((v) => ({ value: v, label: String(v) })),
  },
  {
    key: "time_limit",
    label: "Time limit",
    help: "Stops the search after this long and shows what was found so far. You can also stop it at any moment.",
    choices: [
      { value: 0, label: "none" },
      { value: 10, label: "10 seconds" },
      { value: 30, label: "30 seconds" },
      { value: 60, label: "1 minute" },
      { value: 120, label: "2 minutes" },
      { value: 300, label: "5 minutes" },
    ],
  },
];

export const ADVANCED_DEFAULTS: Advanced = { min_match: 90, rel_tolerance: 1e-6, max_inputs: 5, time_limit: 0 };

/** Every operation, with how it is shown on the page. */
export const OPERATORS: { id: string; label: string }[] = [
  { id: "+", label: "+ add" },
  { id: "-", label: "− subtract" },
  { id: "*", label: "× multiply" },
  { id: "/", label: "÷ divide" },
  { id: "^", label: "xⁿ powers" },
  { id: "sqrt", label: "√ square root" },
  { id: "exp", label: "exp" },
  { id: "log", label: "log" },
  { id: "sin", label: "sin" },
  { id: "cos", label: "cos" },
];

/**
 * What each profile changes in the search. Thorough first searches like
 * Normal and only tries longer formulas (`deepen_to`) for the columns that
 * are still unexplained, which is several times faster than searching every
 * column with long formulas.
 */
export const PROFILES: Record<Profile, { max_size: number; max_params: number; small_samples: number; deepen_to?: number }> = {
  fast: { max_size: 4, max_params: 1, small_samples: 0 },
  normal: { max_size: 5, max_params: 1, small_samples: 4 },
  thorough: { max_size: 5, max_params: 1, small_samples: 4, deepen_to: 6 },
};

export const DEFAULTS: Settings = {
  source: "search",
  domain: "",
  profile: "normal",
  operators: OPERATORS.map((o) => o.id),
  advanced: { ...ADVANCED_DEFAULTS },
};

const KEY = "regrex.settings";

/** The saved settings, or the defaults. Storage may be unavailable (private windows). */
export function loadSettings(): Settings {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<Settings> | null;
    if (!saved) return structuredClone(DEFAULTS);
    const profile = saved.profile && saved.profile in PROFILES ? saved.profile : DEFAULTS.profile;
    const known = new Set(OPERATORS.map((o) => o.id));
    const operators = Array.isArray(saved.operators) ? saved.operators.filter((o) => known.has(o)) : DEFAULTS.operators;
    const advanced = { ...ADVANCED_DEFAULTS };
    for (const { key, choices } of ADVANCED) {
      const v = (saved.advanced as Partial<Advanced> | undefined)?.[key];
      if (choices.some((c) => c.value === v)) advanced[key] = v as number;
    }
    const source: Source = saved.source === "library" ? "library" : "search";
    const domain = typeof saved.domain === "string" ? saved.domain : "";
    return { source, domain, profile, operators, advanced };
  } catch {
    return structuredClone(DEFAULTS);
  }
}

export function saveSettings(settings: Settings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // Not saved: the settings still apply until the page is closed.
  }
}

/** Search options for the engine (the time limit is applied by the page). */
export function searchOptions(settings: Settings) {
  const all = settings.operators.length === OPERATORS.length;
  const { min_match, rel_tolerance, max_inputs } = settings.advanced;
  if (settings.source === "library") {
    return { library_only: true, domain: settings.domain || undefined, min_match: min_match / 100, rel_tolerance };
  }
  return {
    ...PROFILES[settings.profile],
    ...(all ? {} : { operators: settings.operators }),
    min_match: min_match / 100,
    rel_tolerance,
    max_inputs,
  };
}

/** Advanced limits that differ from the defaults. */
export function changedLimits(settings: Settings): (keyof Advanced)[] {
  return ADVANCED.map((a) => a.key).filter((k) => settings.advanced[k] !== ADVANCED_DEFAULTS[k]);
}

/** A short description of the settings, such as "Thorough, 6 of 10 operations, 1 advanced change" or "Library: Finance and business". */
export function describe(settings: Settings, domainName?: string): string {
  const parts: string[] = [];
  if (settings.source === "library") {
    parts.push(domainName ? `Library: ${domainName}` : "Library only");
  } else {
    parts.push({ fast: "Fast", normal: "Normal", thorough: "Thorough" }[settings.profile]);
    const n = settings.operators.length;
    if (n !== OPERATORS.length) parts.push(`${n} of ${OPERATORS.length} operations`);
  }
  const changed = changedLimits(settings).length;
  if (changed) parts.push(`${changed} advanced ${changed === 1 ? "change" : "changes"}`);
  return parts.join(", ");
}

/** The label of the default choice of an advanced limit, such as "90 %". */
export function defaultLabel(key: keyof Advanced): string {
  const a = ADVANCED.find((x) => x.key === key)!;
  return a.choices.find((c) => c.value === ADVANCED_DEFAULTS[key])!.label;
}
