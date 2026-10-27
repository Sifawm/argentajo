import { invoke } from "@tauri-apps/api/core";
import type { TFunction } from "i18next";
import { createContext, useContext } from "react";
import { isLanguage, type Language } from "./language";
import { type JobSettings, NEW_SHEET, PAPERS } from "./job";

export const UNITS = ["mm", "cm", "px"] as const;
/** How lengths are shown and typed; all geometry stays in mm. */
export type Unit = (typeof UNITS)[number];

/** mm per unit; px are CSS px, 96 per inch. */
const MM_PER: Record<Unit, number> = { mm: 1, cm: 10, px: 25.4 / 96 };

/** Rounded to 3 decimals, far below what can be cut, so a typed value comes back as typed. */
export const toUnit = (mm: number, unit: Unit) => Number((mm / MM_PER[unit]).toFixed(3));
export const fromUnit = (value: number, unit: Unit) => value * MM_PER[unit];

/** Decimals shown for a size, per unit. */
const DIGITS: Record<Unit, number> = { mm: 1, cm: 2, px: 0 };

/** "w × h unit", for a size in mm. */
export const formatSize = (t: TFunction, [w, h]: [number, number], unit: Unit) => {
  const digits = { maximumFractionDigits: DIGITS[unit] };
  return t("common:size", {
    w: toUnit(w, unit),
    h: toUnit(h, unit),
    // Not `unit`: i18next hands every option to `Intl.NumberFormat`, which rejects "mm".
    symbol: unit,
    formatParams: { w: digits, h: digits },
  });
};

/** What a new Job starts with. Sizes in mm, the time limit in seconds. */
export interface JobDefaults {
  paper: keyof typeof PAPERS;
  landscape: boolean;
  printerMargin: number;
  separation: number;
  edgeMargin: number;
  timeLimit: number;
}

export interface Preferences {
  unit: Unit;
  /** Null follows the system language. */
  language: Language | null;
  job: JobDefaults;
}

export const FACTORY: Preferences = {
  unit: "mm",
  language: null,
  job: {
    paper: "a4",
    landscape: false,
    printerMargin: 5,
    separation: 1,
    edgeMargin: 1.5,
    timeLimit: 60,
  },
};

export const isSpacing = (n: unknown): n is number =>
  typeof n === "number" && Number.isFinite(n) && n >= 0;
export const isPositive = (n: unknown): n is number => isSpacing(n) && n > 0;

/** The saved preferences, each missing or bad value replaced by its factory default. */
export function parsePreferences(saved: unknown): Preferences {
  const s = (typeof saved === "object" && saved) || {};
  const j = ("job" in s && typeof s.job === "object" && s.job) || {};
  const pick = <T>(object: object, key: string, valid: (v: unknown) => boolean, fallback: T) => {
    const value: unknown = (object as Record<string, unknown>)[key];
    return valid(value) ? (value as T) : fallback;
  };
  const f = FACTORY.job;
  return {
    unit: pick(s, "unit", (v) => UNITS.includes(v as Unit), FACTORY.unit),
    language: pick(s, "language", (v) => isLanguage(v as string), FACTORY.language),
    job: {
      paper: pick(j, "paper", (v) => Object.hasOwn(PAPERS, v as string), f.paper),
      landscape: pick(j, "landscape", (v) => typeof v === "boolean", f.landscape),
      printerMargin: pick(j, "printerMargin", isSpacing, f.printerMargin),
      separation: pick(j, "separation", isSpacing, f.separation),
      edgeMargin: pick(j, "edgeMargin", isSpacing, f.edgeMargin),
      timeLimit: pick(j, "timeLimit", isPositive, f.timeLimit),
    },
  };
}

export const newJob = (defaults: JobDefaults): JobSettings => ({
  ...defaults,
  customPaper: [...PAPERS[defaults.paper]],
  sheets: [NEW_SHEET],
  rotationStep: 1,
  items: [],
});

/** Saved as JSON in app data; factory defaults until then. */
export const loadPreferences = () =>
  invoke<unknown>("load_preferences").then(parsePreferences, () => FACTORY);

// Chained, so saves land in order however fast they come.
let saved: Promise<unknown> = Promise.resolve();
export const savePreferences = (preferences: Preferences) => {
  const save = saved.catch(() => {}).then(() => invoke("save_preferences", { preferences }));
  saved = save;
  return save;
};

export const PreferencesContext = createContext<Preferences>(FACTORY);
export const usePreferences = () => useContext(PreferencesContext);
