import { Channel, invoke } from "@tauri-apps/api/core";
import { NORMAL_SIZE_ID } from "./library";
import type { PieceView } from "./library";

/** Width × height in mm. */
export type Size = [number, number];

/** Paper presets, portrait, in mm. */
export const PAPERS = {
  a3: [297, 420],
  a4: [210, 297],
  a5: [148, 210],
  letter: [215.9, 279.4],
} as const satisfies Record<string, [number, number]>;

/** A new Job's Sheet, in mm. */
export const NEW_SHEET: Size = [100, 50];

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
  /**
   * Pieces from the Library or the Job file opened last, each at one of their Sizes, with a fixed
   * quantity or "fill" for as many as fit in the silver left.
   */
  items: { id: string; size_id: string; quantity: { fixed: number } | "fill"; fixed?: boolean }[];
}

export type PaperName = keyof typeof PAPERS | "custom";
/** The quantity is kept while the item is on fill, for when it's switched back. */
export type Item = {
  id: string;
  size: string;
  quantity: number;
  fill: boolean;
  /** True when the Item came from a reopened Job file and its Size must stay fixed. */
  fixed?: boolean;
};

/** A Job as it's edited. Sizes in mm, the time limit in seconds. */
export interface JobSettings {
  paper: PaperName;
  /** Either way round: the orientation decides. */
  customPaper: Size;
  landscape: boolean;
  printerMargin: number;
  sheets: Size[];
  separation: number;
  edgeMargin: number;
  timeLimit: number;
  items: Item[];
}

/** What `nest_job` and `saveJob` take: the paper oriented, quantities as `core` has them. */
export const toJobRequest = (settings: JobSettings): JobRequest => {
  const { paper, customPaper, landscape } = settings;
  const [shortSide, longSide] = [...(paper === "custom" ? customPaper : PAPERS[paper])].sort(
    (a, b) => a - b,
  );
  return {
    paper: {
      sizeMm: landscape ? [longSide, shortSide] : [shortSide, longSide],
      marginMm: settings.printerMargin,
    },
    sheetsMm: settings.sheets,
    separationMm: settings.separation,
    edgeMarginMm: settings.edgeMargin,
    timeLimitS: settings.timeLimit,
    items: settings.items.map(({ id, size, quantity, fill, fixed }) => ({
      id,
      size_id: size ?? NORMAL_SIZE_ID,
      quantity: fill ? "fill" : { fixed: quantity },
      fixed: fixed ?? false,
    })),
  };
};

/** The settings `toJobRequest` makes `job` from, such as a reopened Job file's. */
export const fromJobRequest = (job: JobRequest): JobSettings => {
  const [w, h] = job.paper.sizeMm;
  const sides: Size = [Math.min(w, h), Math.max(w, h)];
  const preset = (Object.keys(PAPERS) as (keyof typeof PAPERS)[]).find((name) =>
    PAPERS[name].every((side, i) => side === sides[i]),
  );
  return {
    paper: preset ?? "custom",
    customPaper: sides,
    landscape: w > h,
    printerMargin: job.paper.marginMm,
    sheets: job.sheetsMm,
    separation: job.separationMm,
    edgeMargin: job.edgeMarginMm,
    timeLimit: job.timeLimitS,
    items: job.items.map(({ id, size_id, quantity, fixed }) =>
      quantity === "fill"
        ? { id, size: size_id, quantity: 1, fill: true, fixed }
        : { id, size: size_id, quantity: quantity.fixed, fill: false, fixed },
    ),
  };
};

/** Returns the first Size's id, the Default size. */
export const defaultSizeId = (piece: PieceView): string => piece.sizes[0]?.id ?? NORMAL_SIZE_ID;

/** Adds `piece` at its Default size, or bumps the quantity if that Item is already in the Job. */
export const addPiece = (items: Item[], piece: PieceView): Item[] => {
  const id = piece.id;
  if (!id) return items;
  const size = defaultSizeId(piece);
  const index = items.findIndex((item) => item.id === id && item.size === size);
  if (index === -1) return [...items, { id, size, quantity: 1, fill: false }];
  return items.map((item, i) => (i === index ? { ...item, quantity: item.quantity + 1 } : item));
};

/** Changes the Size of the Item keyed by `itemKey`. On collision with an existing Item the two are
 * merged: quantities are summed and fill is kept if either was fill. */
export const changeItemSize = (
  items: Item[],
  itemKey: { id: string; size: string },
  newSizeId: string,
  pieces: PieceView[],
): Item[] => {
  if (itemKey.size === newSizeId) return items;
  const piece = pieces.find((p) => p.id === itemKey.id);
  if (!piece || !piece.sizes.some((s) => s.id === newSizeId)) return items;
  const targetIndex = items.findIndex(
    (item) => item.id === itemKey.id && item.size === itemKey.size,
  );
  if (targetIndex === -1) return items;
  const target = items[targetIndex];
  const mergeIndex = items.findIndex((item) => item.id === itemKey.id && item.size === newSizeId);
  if (mergeIndex === -1) {
    return [
      ...items.slice(0, targetIndex),
      { ...target, size: newSizeId },
      ...items.slice(targetIndex + 1),
    ];
  }
  const merge = items[mergeIndex];
  const merged: Item = {
    ...merge,
    quantity: merge.quantity + target.quantity,
    fill: merge.fill || target.fill,
  };
  return items
    .map((item, i) => {
      if (i === mergeIndex) return merged;
      if (i === targetIndex) return undefined;
      return item;
    })
    .filter((item): item is Item => item !== undefined);
};

/** Adds the first Size of `piece` not yet in the Job, with quantity 1 and fill false. */
export const addAnotherSize = (items: Item[], piece: PieceView): Item[] | undefined => {
  const id = piece.id;
  if (!id) return undefined;
  const used = new Set(items.filter((item) => item.id === id).map((item) => item.size));
  const next = piece.sizes.find((s) => !used.has(s.id));
  if (!next) return undefined;
  return [...items, { id, size: next.id, quantity: 1, fill: false }];
};

/** Removes Items whose Piece or Size no longer exists in `pieces`. */
export const dropMissingItems = (items: Item[], pieces: PieceView[]): Item[] =>
  items.filter((item) => {
    const piece = pieces.find((p) => p.id === item.id);
    return piece !== undefined && piece.sizes.some((s) => s.id === item.size);
  });

/** Mirrors `core::output::OutputOptions`: what's printed besides the pieces and the 1 cm square. */
export interface OutputOptions {
  sheetOutline: boolean;
  silhouette: boolean;
}

export const DEFAULT_OUTPUT: OutputOptions = { sheetOutline: true, silhouette: false };

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
 * `onIterations` the iterations tried so far, four times a second. Pages are rendered with `options`
 * until `setOutputOptions` changes them.
 */
export const nestJob = (
  job: JobRequest,
  options: OutputOptions,
  onProgress: (view: NestView) => void,
  onIterations: (iterations: number) => void,
) => {
  const progress = new Channel<NestView>();
  progress.onmessage = onProgress;
  const iterations = new Channel<number>();
  iterations.onmessage = onIterations;
  return invoke<NestView>("nest_job", {
    job,
    options,
    onProgress: progress,
    onIterations: iterations,
  });
};

/** Ends the run in progress: `nestJob` then resolves with the best Layout so far. */
export const stopNesting = () => invoke("stop_nesting");

/** Renders the latest Layout's pages again with `options`, which later ones are rendered with too. */
export const setOutputOptions = (options: OutputOptions) =>
  invoke<string[]>("set_output_options", { options });

/** Asks where to save the latest Layout as a PDF and saves it there. False if cancelled. */
export const exportPdf = () => invoke<boolean>("export_job_pdf");

/**
 * Asks where to save the Job, and saves it there with its own copy of its pieces, the output options
 * and the latest Layout, unless that was calculated for other settings. False if cancelled.
 */
export const saveJob = (job: JobRequest) => invoke<boolean>("save_job_file", { job });

/** A Job file as reopened: `job` refers to `pieces`, the copies saved with it, by their ids. */
export interface OpenedJob {
  job: JobRequest;
  pieces: PieceView[];
  output: OutputOptions;
  /** The Layout saved with it, rendered with `output`, if any. */
  result: NestView | null;
}

/** Asks for a Job file and opens it, stopping any run. Null if cancelled; rejects a bad file. */
export const openJob = () => invoke<OpenedJob | null>("open_job_file");

/** "My sheets": Sheet sizes saved for reuse, width × height in mm. */
export const loadMySheets = () => invoke<Size[]>("load_my_sheets");

export const saveMySheets = (sheets: Size[]) => invoke("save_my_sheets", { sheets });
