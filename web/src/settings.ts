// Search settings: a profile (fast, normal, thorough) and the operations
// formulas may use. Saved in the browser so they survive a reload.

export type Profile = "fast" | "normal" | "thorough";

export interface Settings {
  profile: Profile;
  /** Operations formulas may use, as accepted by the engine. */
  operators: string[];
}

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

/** What each profile changes in the search. */
export const PROFILES: Record<Profile, { max_size: number; max_params: number; small_samples: number }> = {
  fast: { max_size: 4, max_params: 1, small_samples: 0 },
  normal: { max_size: 5, max_params: 1, small_samples: 4 },
  thorough: { max_size: 6, max_params: 1, small_samples: 4 },
};

export const DEFAULTS: Settings = { profile: "normal", operators: OPERATORS.map((o) => o.id) };

const KEY = "regrex.settings";

/** The saved settings, or the defaults. Storage may be unavailable (private windows). */
export function loadSettings(): Settings {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<Settings> | null;
    if (!saved) return { ...DEFAULTS };
    const profile = saved.profile && saved.profile in PROFILES ? saved.profile : DEFAULTS.profile;
    const known = new Set(OPERATORS.map((o) => o.id));
    const operators = Array.isArray(saved.operators) ? saved.operators.filter((o) => known.has(o)) : DEFAULTS.operators;
    return { profile, operators };
  } catch {
    return { ...DEFAULTS };
  }
}

export function saveSettings(settings: Settings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // Not saved: the settings still apply until the page is closed.
  }
}

/** Search options for the engine. */
export function searchOptions(settings: Settings) {
  const all = settings.operators.length === OPERATORS.length;
  return { ...PROFILES[settings.profile], ...(all ? {} : { operators: settings.operators }) };
}

/** A short description of non-default settings, such as "Thorough, 6 of 10 operations". */
export function describe(settings: Settings): string {
  const name = { fast: "Fast", normal: "Normal", thorough: "Thorough" }[settings.profile];
  const n = settings.operators.length;
  return n === OPERATORS.length ? name : `${name}, ${n} of ${OPERATORS.length} operations`;
}
