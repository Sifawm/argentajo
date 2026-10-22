import { describe, expect, it } from "vitest";
import { NORMAL_SIZE_ID, type PieceView, type Size } from "./library";
import {
  addAnotherSize,
  addPiece,
  changeItemSize,
  defaultSizeId,
  dropMissingItems,
  fromJobRequest,
  type Item,
  type JobRequest,
  toJobRequest,
} from "./job";

const pieceView = (id: string, sizes: Size[]): PieceView => ({
  id,
  image: "",
  sizePx: [100, 100],
  offsetPx: [0, 0],
  sourcePx: [100, 100],
  sizeMm: [10, 10],
  ppi: 300,
  lowResolution: false,
  contours: [],
  allowRotation: true,
  allowMirror: false,
  sizes,
});

const item = (id: string, size = NORMAL_SIZE_ID, quantity = 1, fill = false): Item => ({
  id,
  size,
  quantity,
  fill,
});

const request = (sizeMm: [number, number]): JobRequest => ({
  paper: { sizeMm, marginMm: 4 },
  sheetsMm: [
    [80, 50],
    [60, 40],
  ],
  separationMm: 1.2,
  edgeMarginMm: 2,
  timeLimitS: 30,
  items: [
    { id: "job-0", size_id: "normal", quantity: { fixed: 3 } },
    { id: "job-1", size_id: "large", quantity: "fill" },
  ],
});

describe("fromJobRequest", () => {
  it("gives back settings that make the very same request", () => {
    // Compared as JSON, as a Layout's request is to tell whether it's stale.
    for (const size of [
      [210, 297],
      [297, 210],
      [100, 150],
      [150, 100],
    ] as [number, number][]) {
      expect(JSON.stringify(toJobRequest(fromJobRequest(request(size))))).toBe(
        JSON.stringify(request(size)),
      );
    }
  });

  it("recognises the paper presets and their orientation", () => {
    expect(fromJobRequest(request([297, 210]))).toMatchObject({ paper: "a4", landscape: true });
    expect(fromJobRequest(request([215.9, 279.4]))).toMatchObject({
      paper: "letter",
      landscape: false,
    });
    expect(fromJobRequest(request([150, 100]))).toMatchObject({
      paper: "custom",
      customPaper: [100, 150],
      landscape: true,
    });
  });

  it("gives fill items a quantity of 1 for when they're switched back", () => {
    expect(fromJobRequest(request([210, 297])).items).toEqual([
      { id: "job-0", size: "normal", quantity: 3, fill: false },
      { id: "job-1", size: "large", quantity: 1, fill: true },
    ]);
  });
});

describe("defaultSizeId", () => {
  it("returns the first Size's id", () => {
    const piece = pieceView("p1", [
      { id: "large", name: "Large", measure: { width: 30 } },
      { id: "small", name: "Small", measure: { width: 15 } },
    ]);
    expect(defaultSizeId(piece)).toBe("large");
  });
});

describe("addPiece", () => {
  it("adds a Piece at its Default size", () => {
    const piece = pieceView("p1", [
      { id: "large", name: "Large", measure: { width: 30 } },
      { id: "small", name: "Small", measure: { width: 15 } },
    ]);
    expect(addPiece([], piece)).toEqual([item("p1", "large", 1, false)]);
  });

  it("bumps the quantity when the same (id, size) is already present", () => {
    const piece = pieceView("p1", [{ id: "normal", name: "Normal", measure: { width: 20 } }]);
    const items = [item("p1", "normal", 2, false)];
    expect(addPiece(items, piece)).toEqual([item("p1", "normal", 3, false)]);
  });
});

describe("changeItemSize", () => {
  it("changes an Item's Size", () => {
    const piece = pieceView("p1", [
      { id: "normal", name: "Normal", measure: { width: 20 } },
      { id: "large", name: "Large", measure: { width: 30 } },
    ]);
    const items = [item("p1", "normal", 2, false)];
    expect(changeItemSize(items, { id: "p1", size: "normal" }, "large", [piece])).toEqual([
      item("p1", "large", 2, false),
    ]);
  });

  it("merges on collision, summing quantities and keeping fill", () => {
    const piece = pieceView("p1", [
      { id: "normal", name: "Normal", measure: { width: 20 } },
      { id: "large", name: "Large", measure: { width: 30 } },
    ]);
    const items = [item("p1", "normal", 2, false), item("p1", "large", 3, true)];
    expect(changeItemSize(items, { id: "p1", size: "normal" }, "large", [piece])).toEqual([
      item("p1", "large", 5, true),
    ]);
  });

  it("does nothing when the new Size does not exist", () => {
    const piece = pieceView("p1", [{ id: "normal", name: "Normal", measure: { width: 20 } }]);
    const items = [item("p1", "normal", 1, false)];
    expect(changeItemSize(items, { id: "p1", size: "normal" }, "missing", [piece])).toEqual(items);
  });
});

describe("addAnotherSize", () => {
  it("adds the first Size not yet in the Job", () => {
    const piece = pieceView("p1", [
      { id: "normal", name: "Normal", measure: { width: 20 } },
      { id: "large", name: "Large", measure: { width: 30 } },
      { id: "small", name: "Small", measure: { width: 15 } },
    ]);
    const items = [item("p1", "normal", 2, false)];
    expect(addAnotherSize(items, piece)).toEqual([
      item("p1", "normal", 2, false),
      item("p1", "large", 1, false),
    ]);
  });

  it("returns undefined when every Size is already present", () => {
    const piece = pieceView("p1", [
      { id: "normal", name: "Normal", measure: { width: 20 } },
      { id: "large", name: "Large", measure: { width: 30 } },
    ]);
    const items = [item("p1", "normal", 1, false), item("p1", "large", 1, false)];
    expect(addAnotherSize(items, piece)).toBeUndefined();
  });
});

describe("dropMissingItems", () => {
  it("removes Items whose Piece is gone", () => {
    const items = [item("p1"), item("p2")];
    const pieces = [pieceView("p1", [{ id: "normal", name: "Normal", measure: { width: 20 } }])];
    expect(dropMissingItems(items, pieces)).toEqual([item("p1")]);
  });

  it("removes Items whose Size is gone", () => {
    const items = [item("p1", "normal"), item("p1", "large")];
    const pieces = [pieceView("p1", [{ id: "normal", name: "Normal", measure: { width: 20 } }])];
    expect(dropMissingItems(items, pieces)).toEqual([item("p1", "normal")]);
  });
});
