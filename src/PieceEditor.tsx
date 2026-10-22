import {
  ArrowDown,
  ArrowUp,
  Brush,
  Eraser,
  Maximize,
  Redo2,
  Trash2,
  TriangleAlert,
  Undo2,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { Dialog, Slider, ToggleGroup } from "radix-ui";
import {
  type PointerEvent,
  type ReactNode,
  type WheelEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  addToLibrary,
  type Edits,
  MIN_PPI,
  type PieceView,
  type PrepareOptions,
  previewPiece,
  splitToLibrary,
  type SizeInput,
  updatePiece,
} from "./library";
import { formatSize, fromUnit, toUnit, usePreferences } from "./preferences";
import { createStrokeHistory, type Stroke } from "./strokeHistory";

type Side = "width" | "height";
type Tool = "brush" | "eraser";

// Green outweighs red for the brush, not for the eraser: the convention `core` reads (`PrepareOptions::edits`).
const TOOL_COLOR: Record<Tool, string> = { brush: "#22c55e", eraser: "#ef4444" };
const PADDING = 16; // 1rem each side; the spec's 2rem total margin around the image.
const MIN_BRUSH = 2;
const MAX_BRUSH = 100;

interface Props {
  image: Uint8Array<ArrayBuffer>;
  /** MIME type of `image`, so the browser can show it (SVGs need it). */
  type: string;
  /** A Library piece reopened with its saved settings; saving then overwrites it in place. */
  saved?: { id: string; options: PrepareOptions; edits?: Edits };
  onClose: () => void;
  /** One piece, or one per Part when split. */
  onSaved: (pieces: PieceView[]) => void;
}

type EditorSize = {
  id: string;
  name: string;
  side: Side;
  /** Typed measure in the display unit; empty means "derive from the image" (only valid for the first size). */
  text: string;
};

const newId = () => crypto.randomUUID();

const formatMeasure = (mm: number, unit: Parameters<typeof toUnit>[1]) =>
  String(Number(toUnit(mm, unit).toFixed(2)));

const toEditorSize = (size: SizeInput, unit: Parameters<typeof toUnit>[1]): EditorSize => {
  if (size.measure) {
    return "width" in size.measure
      ? {
          id: size.id,
          name: size.name,
          side: "width",
          text: formatMeasure(size.measure.width, unit),
        }
      : {
          id: size.id,
          name: size.name,
          side: "height",
          text: formatMeasure(size.measure.height, unit),
        };
  }
  return { id: size.id, name: size.name, side: "width", text: "" };
};

const toMeasure = (
  size: EditorSize,
  unit: Parameters<typeof fromUnit>[1],
): SizeInput["measure"] => {
  const text = size.text.trim();
  if (text === "") return undefined;
  const value = Number(text);
  if (Number.isNaN(value)) return undefined;
  const mm = fromUnit(value, unit);
  if (!(mm > 0)) return undefined;
  return size.side === "width" ? { width: mm } : { height: mm };
};

const initialSizes = (
  savedSizes: SizeInput[] | undefined,
  unit: Parameters<typeof toUnit>[1],
): EditorSize[] => {
  if (savedSizes && savedSizes.length > 0) return savedSizes.map((s) => toEditorSize(s, unit));
  return [{ id: newId(), name: "Normal", side: "width", text: "" }];
};

export default function PieceEditor({ image, type, saved, onClose, onSaved }: Props) {
  const { t } = useTranslation("library");
  // The editor is modal, so the unit can't change under the typed sizes.
  const { unit } = usePreferences();
  const [tolerance, setTolerance] = useState(saved?.options.tolerance ?? 32);
  const [editorInitial] = useState(() => {
    const sizes = initialSizes(saved?.options.sizes, unit);
    return { sizes, selectedSizeId: sizes[0]?.id };
  });
  const [sizes, setSizes] = useState<EditorSize[]>(editorInitial.sizes);
  const [selectedSizeId, setSelectedSizeId] = useState<string | undefined>(
    editorInitial.selectedSizeId,
  );
  const [allowRotation, setAllowRotation] = useState(saved?.options.allowRotation ?? true);
  const [allowMirror, setAllowMirror] = useState(saved?.options.allowMirror ?? false);
  const [preview, setPreview] = useState<PieceView>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [sourceUrl, setSourceUrl] = useState<string>();
  const [tool, setTool] = useState<Tool>();
  const [brushSize, setBrushSize] = useState(20);
  const [edits, setEdits] = useState(saved?.edits);
  const [zoom, setZoom] = useState(1);
  const [fittedSize, setFittedSize] = useState<{ width: number; height: number; scale: number }>();
  const [cursor, setCursor] = useState<{ x: number; y: number }>();
  const [cursorVisible, setCursorVisible] = useState(false);
  const [isPanning, setIsPanning] = useState(false);
  const [spacePressed, setSpacePressed] = useState(false);
  const [, setHistoryTick] = useState(0);

  const containerRef = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const baseBitmap = useRef<ImageBitmap | undefined>(undefined);
  const savedEdits = useRef(saved?.edits);
  const historyRef = useRef(createStrokeHistory());
  const currentStroke = useRef<{ tool: Tool; width: number; points: DOMPoint[] } | undefined>(
    undefined,
  );
  const lastPoint = useRef<DOMPoint | undefined>(undefined);
  const isPanningRef = useRef(false);
  const panStart = useRef<
    { scrollLeft: number; scrollTop: number; x: number; y: number } | undefined
  >(undefined);
  const spacePressedRef = useRef(false);
  const cursorVisibleRef = useRef(false);
  const toolRef = useRef<Tool | undefined>(tool);
  const brushSizeRef = useRef(brushSize);
  const zoomRef = useRef(zoom);
  const fitScaleRef = useRef(fittedSize?.scale ?? 1);

  useEffect(() => {
    toolRef.current = tool;
  }, [tool]);
  useEffect(() => {
    brushSizeRef.current = brushSize;
  }, [brushSize]);
  useEffect(() => {
    zoomRef.current = zoom;
  }, [zoom]);
  useEffect(() => {
    if (fittedSize) fitScaleRef.current = fittedSize.scale;
  }, [fittedSize]);

  useEffect(() => {
    const url = URL.createObjectURL(new Blob([image], { type }));
    setSourceUrl(url);
    return () => URL.revokeObjectURL(url);
  }, [image, type]);

  // Keep the selected size valid when rows are deleted or reordered.
  useEffect(() => {
    if (selectedSizeId && sizes.some((s) => s.id === selectedSizeId)) return;
    setSelectedSizeId(sizes[0]?.id);
  }, [sizes, selectedSizeId]);

  const validationErrors = useMemo(() => {
    const errors: Record<string, string> = {};
    const seenIds = new Set<string>();
    const seenNames = new Set<string>();
    sizes.forEach((size, index) => {
      if (seenIds.has(size.id)) {
        errors[size.id] = t("duplicateSizeId");
      }
      seenIds.add(size.id);
      const trimmed = size.name.trim();
      if (trimmed === "") {
        errors[size.id] = t("emptySizeName");
      } else if ([...seenNames].some((n) => n.toLowerCase() === trimmed.toLowerCase())) {
        errors[size.id] = t("duplicateSizeName");
      }
      seenNames.add(trimmed);
      if (index > 0 && !toMeasure(size, unit)) {
        errors[size.id] = t("sizeMeasure");
      }
    });
    return errors;
  }, [sizes, unit, t]);

  const options = useMemo<PrepareOptions>(
    () => ({
      tolerance,
      sizes: sizes.map((s) => ({ id: s.id, name: s.name.trim(), measure: toMeasure(s, unit) })),
      allowRotation,
      allowMirror,
    }),
    [tolerance, sizes, unit, allowRotation, allowMirror],
  );

  // Decoded by `core` (SVGs rasterised), so the overlay and the edits share its pixel grid.
  const [sourceW, sourceH] = preview?.sourcePx ?? [];

  // Fit the source image inside the container with a small margin; zoom scales this wrapper.
  const updateFit = () => {
    const container = containerRef.current;
    if (!container || !sourceW || !sourceH) return;
    const availW = Math.max(0, container.clientWidth - PADDING * 2);
    const availH = Math.max(0, container.clientHeight - PADDING * 2);
    if (availW <= 0 || availH <= 0) return;
    const aspect = sourceW / sourceH;
    let width: number;
    let height: number;
    if (availW / availH > aspect) {
      height = availH;
      width = height * aspect;
    } else {
      width = availW;
      height = width / aspect;
    }
    setFittedSize({ width, height, scale: width / sourceW });
  };

  useEffect(() => {
    updateFit();
    const container = containerRef.current;
    if (!container) return;
    const ro = new ResizeObserver(updateFit);
    ro.observe(container);
    return () => ro.disconnect();
  }, [sourceW, sourceH]);

  // Decode saved edits once and draw them as the non-undoable base layer.
  useEffect(() => {
    if (!sourceW || !sourceH) return;
    const ctx = canvas.current?.getContext("2d");
    if (!ctx) return;
    if (savedEdits.current) {
      void createImageBitmap(new Blob([savedEdits.current], { type: "image/png" })).then(
        (bitmap) => {
          baseBitmap.current = bitmap;
          savedEdits.current = undefined;
          ctx.clearRect(0, 0, sourceW, sourceH);
          ctx.drawImage(bitmap, 0, 0);
        },
      );
    } else {
      ctx.clearRect(0, 0, sourceW, sourceH);
    }
  }, [sourceW, sourceH]);

  // Re-prepare live, debounced so dragging the slider doesn't queue up a run per step.
  useEffect(() => {
    let stale = false;
    const timer = setTimeout(() => {
      previewPiece(image, options, edits).then(
        (piece) => {
          if (stale) return;
          setPreview(piece);
          setError(undefined);
        },
        (e) => {
          if (!stale) setError(String(e));
        },
      );
    }, 150);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [image, options, edits]);

  const exportEdits = () =>
    new Promise<Edits | undefined>((resolve) => {
      if (!canvas.current) return resolve(edits);
      canvas.current.toBlob((png) =>
        png
          ? png.arrayBuffer().then((buffer) => resolve(new Uint8Array(buffer)))
          : resolve(undefined),
      );
    });

  const drawStroke = (ctx: CanvasRenderingContext2D, stroke: Stroke) => {
    if (stroke.points.length === 0) return;
    ctx.strokeStyle = ctx.fillStyle = TOOL_COLOR[stroke.tool];
    ctx.lineWidth = stroke.width;
    ctx.lineCap = ctx.lineJoin = "round";
    const [first, ...rest] = stroke.points;
    ctx.beginPath();
    ctx.arc(first.x, first.y, stroke.width / 2, 0, 2 * Math.PI);
    ctx.fill();
    if (rest.length > 0) {
      ctx.beginPath();
      ctx.moveTo(first.x, first.y);
      for (const p of rest) ctx.lineTo(p.x, p.y);
      ctx.stroke();
    }
  };

  const replay = async () => {
    const ctx = canvas.current?.getContext("2d");
    if (!ctx || !sourceW || !sourceH) return;
    ctx.clearRect(0, 0, sourceW, sourceH);
    if (baseBitmap.current) {
      ctx.drawImage(baseBitmap.current, 0, 0);
    }
    for (const stroke of historyRef.current.inEffect()) {
      drawStroke(ctx, stroke);
    }
    const next = await exportEdits();
    setEdits(next);
  };

  const optionsForSave = (defaultSizeMm: [number, number]): PrepareOptions => {
    const fallbackWidth = defaultSizeMm[0];
    return {
      tolerance,
      sizes: sizes.map((s) => {
        const measure = toMeasure(s, unit);
        return { id: s.id, name: s.name.trim(), measure: measure ?? { width: fallbackWidth } };
      }),
      allowRotation,
      allowMirror,
    };
  };

  const accept = (split: boolean) => {
    if (!preview) return;
    const saveOptions = optionsForSave(preview.sizeMm);
    setSaving(true);
    void exportEdits()
      .then((edits) =>
        split
          ? splitToLibrary(image, saveOptions, edits)
          : (saved
              ? updatePiece(saved.id, image, type, saveOptions, edits)
              : addToLibrary(image, type, saveOptions, edits)
            ).then((piece) => [piece]),
      )
      .then(onSaved, (e) => {
        setError(String(e));
        setSaving(false);
      });
  };

  const doUndo = () => {
    if (!historyRef.current.undo()) return;
    setHistoryTick((n) => n + 1);
    void replay();
  };

  const doRedo = () => {
    if (!historyRef.current.redo()) return;
    setHistoryTick((n) => n + 1);
    void replay();
  };

  const setBrushSizeAndRef = (value: number) => {
    setBrushSize(value);
    brushSizeRef.current = value;
  };

  const adjustBrush = (delta: number) => {
    setBrushSizeAndRef(Math.min(MAX_BRUSH, Math.max(MIN_BRUSH, brushSize + delta)));
  };

  const zoomBy = (ratio: number, center?: { x: number; y: number }) => {
    const container = containerRef.current;
    if (!container || !fittedSize) return;
    const next = Math.min(16, Math.max(1, zoomRef.current * ratio));
    if (next === zoomRef.current) return;
    const px = center?.x ?? container.clientWidth / 2 - PADDING;
    const py = center?.y ?? container.clientHeight / 2 - PADDING;
    const r = next / zoomRef.current;
    container.scrollLeft = (container.scrollLeft + px) * r - px;
    container.scrollTop = (container.scrollTop + py) * r - py;
    zoomRef.current = next;
    setZoom(next);
  };

  const zoomIn = () => zoomBy(1.25);
  const zoomOut = () => zoomBy(1 / 1.25);
  const fit = () => {
    if (!containerRef.current) return;
    zoomRef.current = 1;
    setZoom(1);
    containerRef.current.scrollLeft = 0;
    containerRef.current.scrollTop = 0;
  };

  const handleWheelZoom = (e: WheelEvent<HTMLDivElement>) => {
    const container = containerRef.current;
    if (!container || !fittedSize) return;
    const rect = container.getBoundingClientRect();
    const px = e.clientX - rect.left - PADDING;
    const py = e.clientY - rect.top - PADDING;
    const factor = Math.exp(-e.deltaY * 0.002);
    zoomBy(factor, { x: px, y: py });
  };

  // A dot on press, then a segment per move; the silhouette is re-prepared once the stroke ends.
  const paint = (e: PointerEvent<SVGSVGElement>, from?: DOMPoint) => {
    const ctx = canvas.current?.getContext("2d");
    const toSource = e.currentTarget.getScreenCTM()?.inverse();
    if (!toolRef.current || !ctx || !toSource || !sourceW || !sourceH) return;
    const point = new DOMPoint(e.clientX, e.clientY).matrixTransform(toSource);
    const width = brushSizeRef.current / (fitScaleRef.current * zoomRef.current);
    const paintTool = toolRef.current;
    ctx.strokeStyle = ctx.fillStyle = TOOL_COLOR[paintTool];
    ctx.lineWidth = width;
    ctx.lineCap = ctx.lineJoin = "round";
    ctx.beginPath();
    if (from) {
      ctx.moveTo(from.x, from.y);
      ctx.lineTo(point.x, point.y);
      ctx.stroke();
    } else {
      ctx.arc(point.x, point.y, width / 2, 0, 2 * Math.PI);
      ctx.fill();
    }
    lastPoint.current = point;
  };

  const startStroke = (e: PointerEvent<SVGSVGElement>) => {
    const toSource = e.currentTarget.getScreenCTM()?.inverse();
    if (!toolRef.current || !toSource) return;
    const point = new DOMPoint(e.clientX, e.clientY).matrixTransform(toSource);
    currentStroke.current = {
      tool: toolRef.current,
      width: brushSizeRef.current / (fitScaleRef.current * zoomRef.current),
      points: [point],
    };
    paint(e);
  };

  const endStroke = () => {
    const stroke = currentStroke.current;
    currentStroke.current = undefined;
    lastPoint.current = undefined;
    if (!stroke || stroke.points.length === 0) return;
    historyRef.current.add(stroke);
    setHistoryTick((n) => n + 1);
    void replay();
  };

  const updateCursor = (e: PointerEvent<SVGSVGElement>) => {
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    setCursor({
      x: e.clientX - rect.left - PADDING,
      y: e.clientY - rect.top - PADDING,
    });
  };

  const onPointerDown = (e: PointerEvent<SVGSVGElement>) => {
    if (e.button !== 0 && e.button !== 1) return;
    const canPan = e.button === 1 || spacePressedRef.current || !toolRef.current;
    if (canPan) {
      e.preventDefault();
      isPanningRef.current = true;
      setIsPanning(true);
      panStart.current = {
        scrollLeft: containerRef.current!.scrollLeft,
        scrollTop: containerRef.current!.scrollTop,
        x: e.clientX,
        y: e.clientY,
      };
      e.currentTarget.setPointerCapture(e.pointerId);
      return;
    }
    if (e.button === 0 && toolRef.current) {
      e.currentTarget.setPointerCapture(e.pointerId);
      startStroke(e);
    }
  };

  const onPointerMove = (e: PointerEvent<SVGSVGElement>) => {
    updateCursor(e);
    if (isPanningRef.current) {
      const start = panStart.current;
      const container = containerRef.current;
      if (!start || !container) return;
      container.scrollLeft = start.scrollLeft - (e.clientX - start.x);
      container.scrollTop = start.scrollTop - (e.clientY - start.y);
      return;
    }
    if (!lastPoint.current) return;
    paint(e, lastPoint.current);
    if (currentStroke.current) {
      currentStroke.current.points.push(lastPoint.current);
    }
  };

  const onPointerUp = () => {
    if (isPanningRef.current) {
      isPanningRef.current = false;
      setIsPanning(false);
      panStart.current = undefined;
      return;
    }
    endStroke();
  };

  const onPointerEnter = () => {
    cursorVisibleRef.current = true;
    setCursorVisible(true);
  };

  const onPointerLeave = () => {
    cursorVisibleRef.current = false;
    setCursorVisible(false);
  };

  // Keyboard shortcuts for zoom, brush size and undo/redo; Space is a pan modifier over the image.
  useEffect(() => {
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      const target = e.target;
      const inInput = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;

      if (e.key === " " && cursorVisibleRef.current && !inInput) {
        e.preventDefault();
        spacePressedRef.current = true;
        setSpacePressed(true);
        return;
      }

      if (inInput) return;

      if (e.ctrlKey || e.metaKey) {
        const key = e.key.toLowerCase();
        if (key === "z") {
          e.preventDefault();
          if (e.shiftKey) doRedo();
          else doUndo();
          return;
        }
        if (key === "y") {
          e.preventDefault();
          doRedo();
          return;
        }
      }

      if (e.key === "+" || e.key === "Add") {
        e.preventDefault();
        zoomIn();
      } else if (e.key === "-" || e.key === "Subtract") {
        e.preventDefault();
        zoomOut();
      } else if (e.key === "0") {
        e.preventDefault();
        fit();
      } else if (e.key === "[") {
        e.preventDefault();
        adjustBrush(-2);
      } else if (e.key === "]") {
        e.preventDefault();
        adjustBrush(2);
      }
    };

    const onKeyUp = (e: globalThis.KeyboardEvent) => {
      if (e.key === " ") {
        spacePressedRef.current = false;
        setSpacePressed(false);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, []);

  const containerCursor = useMemo(() => {
    if (isPanning) return "cursor-grabbing";
    if (spacePressed) return "cursor-grab";
    if (!tool) return "cursor-grab";
    return "";
  }, [isPanning, spacePressed, tool]);

  const renderCursor = (cx: number, cy: number) => (
    <g>
      <circle cx={cx} cy={cy} r={brushSize / 2} stroke="white" strokeWidth={2} fill="none" />
      <circle cx={cx} cy={cy} r={brushSize / 2} stroke="black" strokeWidth={1} fill="none" />
      <line x1={cx - 1} y1={cy} x2={cx + 1} y2={cy} stroke="white" strokeWidth={2} />
      <line x1={cx} y1={cy - 1} x2={cx} y2={cy + 1} stroke="white" strokeWidth={2} />
      <line x1={cx - 1} y1={cy} x2={cx + 1} y2={cy} stroke="black" strokeWidth={1} />
      <line x1={cx} y1={cy - 1} x2={cx} y2={cy + 1} stroke="black" strokeWidth={1} />
    </g>
  );

  const slider = (
    label: string,
    value: number,
    min: number,
    max: number,
    onChange: (value: number) => void,
    onPointerDown?: () => void,
    onPointerUp?: () => void,
  ) => (
    <Slider.Root
      min={min}
      max={max}
      value={[value]}
      onValueChange={([value]) => onChange(value)}
      onPointerDown={onPointerDown}
      onPointerUp={onPointerUp}
      onPointerLeave={onPointerUp}
      className="relative flex h-5 touch-none items-center"
    >
      <Slider.Track className="relative h-1 grow rounded-full bg-zinc-200">
        <Slider.Range className="absolute h-full rounded-full bg-zinc-900" />
      </Slider.Track>
      <Slider.Thumb
        aria-label={label}
        className="block size-4 rounded-full border border-zinc-400 bg-white"
      />
    </Slider.Root>
  );

  const toolButton = (value: Tool, icon: ReactNode) => (
    <ToggleGroup.Item
      value={value}
      className="inline-flex flex-1 items-center justify-center gap-2 rounded-md border border-zinc-300 px-3 py-1.5 text-sm data-[state=on]:border-zinc-900 data-[state=on]:bg-zinc-900 data-[state=on]:text-white"
    >
      {icon}
      {t(value)}
    </ToggleGroup.Item>
  );

  const iconButton = (label: string, onClick: () => void, icon: ReactNode, disabled?: boolean) => (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      aria-label={label}
      className="inline-flex flex-1 items-center justify-center gap-2 rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-40"
    >
      {icon}
    </button>
  );

  const updateSize = (id: string, patch: Partial<EditorSize>) => {
    setSizes((sizes) => sizes.map((s) => (s.id === id ? { ...s, ...patch } : s)));
  };

  const makeDefault = (id: string) => {
    setSizes((sizes) => {
      const index = sizes.findIndex((s) => s.id === id);
      if (index <= 0) return sizes;
      const moved = sizes[index];
      return [moved, ...sizes.slice(0, index), ...sizes.slice(index + 1)];
    });
  };

  const moveUp = (id: string) => {
    setSizes((sizes) => {
      const index = sizes.findIndex((s) => s.id === id);
      if (index <= 0) return sizes;
      const next = [...sizes];
      [next[index - 1], next[index]] = [next[index], next[index - 1]];
      return next;
    });
  };

  const moveDown = (id: string) => {
    setSizes((sizes) => {
      const index = sizes.findIndex((s) => s.id === id);
      if (index < 0 || index >= sizes.length - 1) return sizes;
      const next = [...sizes];
      [next[index], next[index + 1]] = [next[index + 1], next[index]];
      return next;
    });
  };

  const deleteSize = (id: string) => {
    if (sizes.length <= 1) return;
    setSizes((sizes) => sizes.filter((s) => s.id !== id));
  };

  const addSize = () => {
    const n = sizes.length + 1;
    const size: EditorSize = {
      id: newId(),
      name: t("newSizeName", { n }),
      side: "width",
      text: "",
    };
    setSizes((sizes) => [...sizes, size]);
    setSelectedSizeId(size.id);
  };

  const selectedSize = sizes.find((s) => s.id === selectedSizeId);
  const selectedDims = useMemo<[number, number] | undefined>(() => {
    if (!preview) return undefined;
    const [w, h] = preview.sizeMm;
    const measure = selectedSize ? toMeasure(selectedSize, unit) : undefined;
    if (!measure) return [w, h];
    const scale = "width" in measure ? measure.width / w : measure.height / h;
    return [w * scale, h * scale];
  }, [preview, selectedSize, unit]);

  const selectedPpi =
    preview && selectedDims ? preview.ppi / (selectedDims[0] / preview.sizeMm[0]) : undefined;

  const cursorOverlayVisible = !isPanning && cursorVisible && !!tool;
  const hasErrors = Object.keys(validationErrors).length > 0;

  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed inset-6 flex flex-col gap-4 rounded-lg bg-white p-6 shadow-xl">
          <Dialog.Title className="text-xl font-semibold">{t("editorTitle")}</Dialog.Title>
          <Dialog.Description className="sr-only">{t("editorDescription")}</Dialog.Description>

          <div className="flex min-h-0 flex-1 gap-6">
            <div
              ref={containerRef}
              className={`checkerboard relative min-w-0 flex-1 overflow-auto rounded-md p-4 ${containerCursor}`}
              onWheel={(e) => {
                if (e.ctrlKey) {
                  e.preventDefault();
                  handleWheelZoom(e);
                }
              }}
            >
              {sourceUrl && preview && fittedSize && (
                <>
                  <div
                    style={{
                      width: fittedSize.width * zoom,
                      height: fittedSize.height * zoom,
                    }}
                    className={`relative ${tool && !isPanning && !spacePressed ? "cursor-none" : ""}`}
                  >
                    <svg
                      viewBox={`0 0 ${sourceW} ${sourceH}`}
                      className="absolute inset-0 size-full touch-none"
                      role="img"
                      aria-label={t("silhouette")}
                      onPointerDown={onPointerDown}
                      onPointerMove={onPointerMove}
                      onPointerUp={onPointerUp}
                      onPointerCancel={onPointerUp}
                      onPointerEnter={onPointerEnter}
                      onPointerLeave={onPointerLeave}
                    >
                      {/* Faded, so what was removed as background can still be brushed back. */}
                      <image href={sourceUrl} width={sourceW} height={sourceH} opacity={0.25} />
                      <g
                        transform={`translate(${preview.offsetPx[0]} ${preview.offsetPx[1]}) scale(${preview.sizePx[0] / preview.sizeMm[0]})`}
                      >
                        <image
                          href={preview.image}
                          width={preview.sizeMm[0]}
                          height={preview.sizeMm[1]}
                        />
                        {preview.contours.map((contour, i) => (
                          <polygon
                            key={i}
                            points={contour.map(([x, y]) => `${x},${y}`).join(" ")}
                            fill="none"
                            stroke="#0ea5e9"
                            strokeWidth={2}
                            vectorEffect="non-scaling-stroke"
                          />
                        ))}
                      </g>
                    </svg>
                    {/* Scaled exactly like the SVG, so it stays pixel-aligned. */}
                    <canvas
                      ref={canvas}
                      width={sourceW}
                      height={sourceH}
                      className="pointer-events-none absolute inset-0 size-full opacity-40"
                    />
                  </div>
                  {cursorOverlayVisible && (
                    <svg
                      className="pointer-events-none absolute inset-4 overflow-hidden"
                      width="100%"
                      height="100%"
                    >
                      {cursorVisible && cursor && renderCursor(cursor.x, cursor.y)}
                    </svg>
                  )}
                </>
              )}
            </div>

            <div className="flex w-72 flex-col gap-5">
              <label className="flex flex-col gap-2 text-sm">
                <span>
                  {t("tolerance")}: {tolerance}
                </span>
                {slider(t("tolerance"), tolerance, 0, 255, setTolerance)}
                <span className="text-xs text-zinc-500">{t("toleranceHint")}</span>
              </label>

              <div className="flex flex-col gap-2 text-sm">
                <span>{t("tools")}</span>
                <ToggleGroup.Root
                  type="single"
                  value={tool ?? ""}
                  onValueChange={(value) => setTool((value || undefined) as Tool | undefined)}
                  className="flex gap-2"
                >
                  {toolButton("brush", <Brush aria-hidden className="size-4" />)}
                  {toolButton("eraser", <Eraser aria-hidden className="size-4" />)}
                </ToggleGroup.Root>
                <div className="flex gap-2">
                  {iconButton(
                    t("undo"),
                    doUndo,
                    <Undo2 aria-hidden className="size-4" />,
                    !historyRef.current.canUndo(),
                  )}
                  {iconButton(
                    t("redo"),
                    doRedo,
                    <Redo2 aria-hidden className="size-4" />,
                    !historyRef.current.canRedo(),
                  )}
                </div>
                <span>
                  {t("brushSize")}: {brushSize}
                </span>
                {slider(
                  t("brushSize"),
                  brushSize,
                  MIN_BRUSH,
                  MAX_BRUSH,
                  setBrushSizeAndRef,
                  () => {},
                  () => {},
                )}
                <span className="text-xs text-zinc-500">{t("toolsHint")}</span>
              </div>

              <div className="flex flex-col gap-2 text-sm">
                <span>{Math.round(zoom * 100)}%</span>
                <div className="flex gap-2">
                  {iconButton(t("zoomOut"), zoomOut, <ZoomOut aria-hidden className="size-4" />)}
                  {iconButton(t("fit"), fit, <Maximize aria-hidden className="size-4" />)}
                  {iconButton(t("zoomIn"), zoomIn, <ZoomIn aria-hidden className="size-4" />)}
                </div>
              </div>

              <div className="flex flex-col gap-2">
                <span className="text-sm">{t("sizes")}</span>
                <div className="flex max-h-56 flex-col gap-2 overflow-y-auto">
                  {sizes.map((size, index) => {
                    const selected = size.id === selectedSizeId;
                    return (
                      <div
                        key={size.id}
                        className={`flex flex-col gap-1 rounded-md border p-2 ${selected ? "border-zinc-900 bg-zinc-50" : "border-zinc-200"}`}
                      >
                        <div className="flex items-center gap-2">
                          <input
                            type="radio"
                            name="selectedSize"
                            checked={selected}
                            onChange={() => setSelectedSizeId(size.id)}
                            aria-label={t("size")}
                            className="shrink-0"
                          />
                          <input
                            type="text"
                            value={size.name}
                            onChange={(e) => updateSize(size.id, { name: e.target.value })}
                            aria-label={t("sizeName")}
                            className="min-w-0 flex-1 rounded-md border border-zinc-300 px-2 py-1 text-sm"
                          />
                          <div className="flex shrink-0 overflow-hidden rounded-md border border-zinc-300">
                            <button
                              type="button"
                              title={t("width", { unit })}
                              onClick={() => updateSize(size.id, { side: "width" })}
                              className={`px-2 py-1 text-xs ${size.side === "width" ? "bg-zinc-900 text-white" : "bg-white hover:bg-zinc-100"}`}
                            >
                              ↔
                            </button>
                            <button
                              type="button"
                              title={t("height", { unit })}
                              onClick={() => updateSize(size.id, { side: "height" })}
                              className={`px-2 py-1 text-xs ${size.side === "height" ? "bg-zinc-900 text-white" : "bg-white hover:bg-zinc-100"}`}
                            >
                              ↕
                            </button>
                          </div>
                          <input
                            type="number"
                            min="0"
                            step="0.1"
                            value={size.text}
                            onChange={(e) => updateSize(size.id, { text: e.target.value })}
                            aria-label={t(size.side, { unit })}
                            className="w-20 rounded-md border border-zinc-300 px-2 py-1 text-sm"
                          />
                          <button
                            type="button"
                            onClick={() => makeDefault(size.id)}
                            disabled={index === 0}
                            title={t("makeDefault")}
                            className="shrink-0 rounded-md border border-zinc-300 px-2 py-1 text-xs hover:bg-zinc-100 disabled:opacity-40"
                          >
                            {t("makeDefault")}
                          </button>
                          <button
                            type="button"
                            onClick={() => moveUp(size.id)}
                            disabled={index === 0}
                            aria-label={t("moveUp")}
                            title={t("moveUp")}
                            className="shrink-0 rounded-md border border-zinc-300 p-1.5 text-zinc-600 hover:bg-zinc-100 disabled:opacity-40"
                          >
                            <ArrowUp aria-hidden className="size-4" />
                          </button>
                          <button
                            type="button"
                            onClick={() => moveDown(size.id)}
                            disabled={index === sizes.length - 1}
                            aria-label={t("moveDown")}
                            title={t("moveDown")}
                            className="shrink-0 rounded-md border border-zinc-300 p-1.5 text-zinc-600 hover:bg-zinc-100 disabled:opacity-40"
                          >
                            <ArrowDown aria-hidden className="size-4" />
                          </button>
                          <button
                            type="button"
                            onClick={() => deleteSize(size.id)}
                            disabled={sizes.length <= 1}
                            aria-label={t("deleteSize")}
                            title={t("deleteSize")}
                            className="shrink-0 rounded-md border border-zinc-300 p-1.5 text-zinc-600 hover:text-red-700 disabled:opacity-40"
                          >
                            <Trash2 aria-hidden className="size-4" />
                          </button>
                        </div>
                        {validationErrors[size.id] && (
                          <p className="text-xs text-red-700">{validationErrors[size.id]}</p>
                        )}
                        {index === 0 && (
                          <span className="text-xs text-zinc-500">{t("defaultSize")}</span>
                        )}
                      </div>
                    );
                  })}
                </div>
                <button
                  type="button"
                  onClick={addSize}
                  className="self-start rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100"
                >
                  {t("addSize")}
                </button>
              </div>

              {selectedDims && (
                <p className="text-sm text-zinc-600">
                  {formatSize(t, selectedDims, unit)} ·{" "}
                  {t("sizePx", { w: preview!.sizePx[0], h: preview!.sizePx[1] })}
                </p>
              )}
              {selectedPpi !== undefined && selectedPpi < MIN_PPI && (
                <p role="status" className="flex gap-2 text-sm text-amber-700">
                  <TriangleAlert aria-hidden className="size-4 shrink-0" />
                  {t("lowResolution", { ppi: Math.round(selectedPpi) })}
                </p>
              )}

              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={allowRotation}
                  onChange={(e) => setAllowRotation(e.target.checked)}
                />
                {t("allowRotation")}
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={allowMirror}
                  onChange={(e) => setAllowMirror(e.target.checked)}
                />
                {t("allowMirror")}
              </label>

              {error && (
                <p role="alert" className="text-sm text-red-700">
                  {t("prepareFailed", { error })}
                </p>
              )}
            </div>
          </div>

          <div className="flex justify-end gap-3">
            <Dialog.Close className="rounded-md px-4 py-2 text-sm hover:bg-zinc-100">
              {t("cancel")}
            </Dialog.Close>
            {/* Only on request: a motif's detached parts usually belong together. */}
            {!saved && preview && preview.contours.length > 1 && (
              <button
                type="button"
                disabled={!preview || hasErrors || saving}
                onClick={() => accept(true)}
                className="rounded-md border border-zinc-300 px-4 py-2 text-sm disabled:opacity-40"
              >
                {t("split", { count: preview.contours.length })}
              </button>
            )}
            <button
              type="button"
              disabled={!preview || hasErrors || saving}
              onClick={() => accept(false)}
              className="rounded-md bg-zinc-900 px-4 py-2 text-sm text-white disabled:opacity-40"
            >
              {t(saved ? "save" : "accept")}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
