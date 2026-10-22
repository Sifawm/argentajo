import { describe, expect, it } from "vitest";
import { createStrokeHistory, type Stroke } from "./strokeHistory";

const stroke = (tool: "brush" | "eraser", n: number): Stroke => ({
  tool,
  width: n,
  points: [{ x: n, y: n }],
});

describe("createStrokeHistory", () => {
  it("has nothing to undo or redo when empty", () => {
    const history = createStrokeHistory();

    expect(history.canUndo()).toBe(false);
    expect(history.canRedo()).toBe(false);
    expect(history.inEffect()).toEqual([]);
    expect(history.undo()).toBe(false);
    expect(history.redo()).toBe(false);
  });

  it("adds strokes and returns them in paint order", () => {
    const history = createStrokeHistory();
    const a = stroke("brush", 1);
    const b = stroke("eraser", 2);

    history.add(a);
    history.add(b);

    expect(history.inEffect()).toEqual([a, b]);
    expect(history.canUndo()).toBe(true);
    expect(history.canRedo()).toBe(false);
  });

  it("undoes and redoes the last stroke", () => {
    const history = createStrokeHistory();
    const a = stroke("brush", 1);

    history.add(a);
    expect(history.undo()).toBe(true);
    expect(history.inEffect()).toEqual([]);
    expect(history.canUndo()).toBe(false);
    expect(history.canRedo()).toBe(true);

    expect(history.redo()).toBe(true);
    expect(history.inEffect()).toEqual([a]);
    expect(history.canRedo()).toBe(false);
  });

  it("undoes several strokes in a row and redoes them in order", () => {
    const history = createStrokeHistory();
    const a = stroke("brush", 1);
    const b = stroke("eraser", 2);
    const c = stroke("brush", 3);

    history.add(a);
    history.add(b);
    history.add(c);

    expect(history.undo()).toBe(true);
    expect(history.undo()).toBe(true);
    expect(history.inEffect()).toEqual([a]);

    expect(history.redo()).toBe(true);
    expect(history.inEffect()).toEqual([a, b]);

    expect(history.redo()).toBe(true);
    expect(history.inEffect()).toEqual([a, b, c]);
    expect(history.canRedo()).toBe(false);
  });

  it("clears the redo stack when adding a stroke after an undo", () => {
    const history = createStrokeHistory();
    const a = stroke("brush", 1);
    const b = stroke("eraser", 2);
    const c = stroke("brush", 3);

    history.add(a);
    history.add(b);
    history.undo();
    history.add(c);

    expect(history.inEffect()).toEqual([a, c]);
    expect(history.canRedo()).toBe(false);
    expect(history.redo()).toBe(false);
  });
});
