import { invoke } from "@tauri-apps/api/core";

/** A named real Size of a Piece: a side in mm, the other follows the proportions. */
export interface Size {
  id: string;
  name: string;
  measure: { width: number } | { height: number };
}

/** A Size as it arrives in preparation options; the first one may omit its measure. */
export interface SizeInput {
  id: string;
  name: string;
  measure?: { width: number } | { height: number };
}

/** Well-known id for the backward-compatible Normal size. */
export const NORMAL_SIZE_ID = "normal";

/** Minimum effective ppi before a Size is flagged as low resolution. Kept in sync with core. */
export const MIN_PPI = 150;

/**
 * Computes the real dimensions of `piece` at `sizeId`. The Default size returns the piece's own
 * `sizeMm`; other Sizes are scaled uniformly by their measure against the Default size.
 */
export const sizeDimensions = (piece: PieceView, sizeId: string): [number, number] | undefined => {
  const size = piece.sizes.find((s) => s.id === sizeId);
  if (!size) return undefined;
  const [w, h] = piece.sizeMm;
  if (sizeId === piece.sizes[0]?.id) return [w, h];
  const scale = size.measure
    ? "width" in size.measure
      ? size.measure.width / w
      : size.measure.height / h
    : 1;
  return [w * scale, h * scale];
};

/** Mirrors `core::piece::PrepareOptions`. Sizes are in mm. */
export interface PrepareOptions {
  tolerance: number;
  /** The Piece's named Sizes. The first is the Default size; a missing measure is derived from DPI. */
  sizes: SizeInput[];
  allowRotation: boolean;
  allowMirror: boolean;
}

/** A Piece with its cleaned PNG as a data URL. `id` is set once it is in the Library. */
export interface PieceView {
  id: string | null;
  image: string;
  sizePx: [number, number];
  /** Top-left of the crop in the source image, in px. */
  offsetPx: [number, number];
  /** Size of the decoded source image (SVGs rasterised): what `offsetPx` and the edits refer to. */
  sourcePx: [number, number];
  sizeMm: [number, number];
  /** Effective resolution at the real size, in px per inch. */
  ppi: number;
  /** Too few ppi for a sharp print and an accurate outline. */
  lowResolution: boolean;
  /** One outer silhouette per Part, in mm, origin top-left, y down. */
  contours: [number, number][][];
  allowRotation: boolean;
  allowMirror: boolean;
  /** Ordered, non-empty list of named Sizes. The first is the Default size. */
  sizes: Size[];
}

/** Manual brush/eraser edits: a PNG the size of the source image, read by `core` (`PrepareOptions::edits`). */
export type Edits = Uint8Array<ArrayBuffer>;

// The image goes as the raw body and the options as a header, so the bytes are not JSON-encoded.
// Edits are appended to the body, with their length in a header.
const prepare = <T = PieceView>(
  command: string,
  image: Uint8Array,
  options: PrepareOptions,
  edits: Edits = new Uint8Array(),
  headers: Record<string, string> = {},
) => {
  const body = new Uint8Array(image.length + edits.length);
  body.set(image);
  body.set(edits, image.length);
  return invoke<T>(command, body, {
    headers: { ...headers, options: JSON.stringify(options), "edits-length": String(edits.length) },
  });
};

export const previewPiece = (image: Uint8Array, options: PrepareOptions, edits?: Edits) =>
  prepare("preview_piece", image, options, edits);

/** `type` is the image's MIME type, kept so the piece can be reopened in the editor. */
export const addToLibrary = (
  image: Uint8Array,
  type: string,
  options: PrepareOptions,
  edits?: Edits,
) => prepare("add_to_library", image, options, edits, { type });

/** Re-prepares Library piece `id` and overwrites it in place. */
export const updatePiece = (
  id: string,
  image: Uint8Array,
  type: string,
  options: PrepareOptions,
  edits?: Edits,
) => prepare("update_piece", image, options, edits, { id, type });

/** One Library piece per Part, all at the scale the first Size in `options.sizes` gives. */
export const splitToLibrary = (image: Uint8Array, options: PrepareOptions, edits?: Edits) =>
  prepare<PieceView[]>("split_to_library", image, options, edits);

export const listLibrary = () => invoke<PieceView[]>("list_library");

/** What the editor reopens a Library piece from. */
export interface OpenedPiece {
  image: Uint8Array<ArrayBuffer>;
  type: string;
  options: PrepareOptions;
  edits?: Edits;
}

const fromBase64 = (base64: string) => Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));

export const openPiece = async (id: string): Promise<OpenedPiece> => {
  const opened = await invoke<{
    image: string;
    type: string;
    options: PrepareOptions;
    edits: string | null;
  }>("open_piece", { id });
  return {
    ...opened,
    image: fromBase64(opened.image),
    edits: opened.edits ? fromBase64(opened.edits) : undefined,
  };
};

export const deletePiece = (id: string) => invoke<void>("delete_piece", { id });
