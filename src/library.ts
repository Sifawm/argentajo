import { invoke } from "@tauri-apps/api/core";

/** Mirrors `core::piece::PrepareOptions`. Sizes are in mm. */
export interface PrepareOptions {
  tolerance: number;
  size: { width: number } | { height: number };
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
  sizeMm: [number, number];
  /** One outer silhouette per Part, in mm, origin top-left, y down. */
  contours: [number, number][][];
  allowRotation: boolean;
  allowMirror: boolean;
}

/** Manual brush/eraser edits: a PNG the size of the source image, read by `core` (`PrepareOptions::edits`). */
export type Edits = Uint8Array;

// The image goes as the raw body and the options as a header, so the bytes are not JSON-encoded.
// Edits are appended to the body, with their length in a header.
const prepare = (
  command: string,
  image: Uint8Array,
  options: PrepareOptions,
  edits: Edits = new Uint8Array(),
) => {
  const body = new Uint8Array(image.length + edits.length);
  body.set(image);
  body.set(edits, image.length);
  return invoke<PieceView>(command, body, {
    headers: { options: JSON.stringify(options), "edits-length": String(edits.length) },
  });
};

export const previewPiece = (image: Uint8Array, options: PrepareOptions, edits?: Edits) =>
  prepare("preview_piece", image, options, edits);

export const addToLibrary = (image: Uint8Array, options: PrepareOptions, edits?: Edits) =>
  prepare("add_to_library", image, options, edits);

export const listLibrary = () => invoke<PieceView[]>("list_library");
