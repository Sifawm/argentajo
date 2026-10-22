export type Tool = "brush" | "eraser";

export interface Point {
  x: number;
  y: number;
}

export interface Stroke {
  tool: Tool;
  width: number;
  points: readonly Point[];
}

export interface StrokeHistory {
  add(stroke: Stroke): void;
  undo(): boolean;
  redo(): boolean;
  canUndo(): boolean;
  canRedo(): boolean;
  inEffect(): readonly Stroke[];
}

export function createStrokeHistory(): StrokeHistory {
  const strokes: Stroke[] = [];
  let pointer = 0;

  return {
    add(stroke) {
      strokes.splice(pointer);
      strokes.push(stroke);
      pointer = strokes.length;
    },

    undo() {
      if (pointer === 0) return false;
      pointer -= 1;
      return true;
    },

    redo() {
      if (pointer === strokes.length) return false;
      pointer += 1;
      return true;
    },

    canUndo() {
      return pointer > 0;
    },

    canRedo() {
      return pointer < strokes.length;
    },

    inEffect() {
      return strokes.slice(0, pointer);
    },
  };
}
