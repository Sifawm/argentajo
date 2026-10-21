import { invoke } from "@tauri-apps/api/core";

/** Paper presets, portrait, in mm. */
export const PAPERS = {
  a3: [297, 420],
  a4: [210, 297],
  a5: [148, 210],
  letter: [215.9, 279.4],
} as const satisfies Record<string, [number, number]>;

/** Mirrors the `nest_job` command's request. Sizes in mm. */
export interface JobRequest {
  /** Oriented as printed. */
  paper: { sizeMm: [number, number]; marginMm: number };
  sheetMm: [number, number];
  separationMm: number;
  edgeMarginMm: number;
  /** Library piece ids with their quantities. */
  items: { id: string; quantity: number }[];
}

/** Mirrors `core::nest::Layout`, with its page rendered by `core` as an SVG in mm. */
export interface NestView {
  svg: string;
  /** Copies placed, per Job item. */
  placed: number[];
  /** Copies that didn't fit, per Job item. */
  missing: number[];
  /** Area of the placed Parts over the Sheet's area, 0–1. */
  utilisation: number;
}

export const nestJob = (job: JobRequest) => invoke<NestView>("nest_job", { job });
