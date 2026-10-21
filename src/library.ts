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
  sizeMm: [number, number];
  /** Outer silhouette in mm, origin top-left, y down. */
  contour: [number, number][];
  allowRotation: boolean;
  allowMirror: boolean;
}

// The image goes as the raw body and the options as a header, so the bytes are not JSON-encoded.
const prepare = (command: string, image: Uint8Array, options: PrepareOptions) =>
  invoke<PieceView>(command, image, { headers: { options: JSON.stringify(options) } });

export const previewPiece = (image: Uint8Array, options: PrepareOptions) =>
  prepare("preview_piece", image, options);

export const addToLibrary = (image: Uint8Array, options: PrepareOptions) =>
  prepare("add_to_library", image, options);

export const listLibrary = () => invoke<PieceView[]>("list_library");
