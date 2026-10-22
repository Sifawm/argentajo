import { Channel, invoke } from "@tauri-apps/api/core";

/** Width × height in mm. */
export type Size = [number, number];

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
  /** The silver Sheets as each lies on the paper, in the order they're filled. */
  sheetsMm: Size[];
  separationMm: number;
  edgeMarginMm: number;
  /** Bound on the nesting run, in seconds. */
  timeLimitS: number;
  /** Library piece ids with a fixed quantity, or "fill" for as many as fit in the silver left. */
  items: { id: string; quantity: { fixed: number } | "fill" }[];
}

/** Mirrors `core::nest::Layout`, with each of its pages rendered by `core` as an SVG in mm. */
export interface NestView {
  pages: string[];
  /** Copies placed, per Job item. */
  placed: number[];
  /** Copies that didn't fit, per Job item. */
  missing: number[];
  /** Area of the placed Parts over the Sheets' area, 0–1. */
  utilisation: number;
}

/**
 * Resolves with the best Layout once the run ends; `onProgress` gets every improved one before, and
 * `onIterations` the iterations tried so far, four times a second.
 */
export const nestJob = (
  job: JobRequest,
  onProgress: (view: NestView) => void,
  onIterations: (iterations: number) => void,
) => {
  const progress = new Channel<NestView>();
  progress.onmessage = onProgress;
  const iterations = new Channel<number>();
  iterations.onmessage = onIterations;
  return invoke<NestView>("nest_job", { job, onProgress: progress, onIterations: iterations });
};

/** Ends the run in progress: `nestJob` then resolves with the best Layout so far. */
export const stopNesting = () => invoke("stop_nesting");

/** "My sheets": Sheet sizes saved for reuse, width × height in mm. */
export const loadMySheets = () => invoke<Size[]>("load_my_sheets");

export const saveMySheets = (sheets: Size[]) => invoke("save_my_sheets", { sheets });
