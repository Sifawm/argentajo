import { Brush, Eraser, TriangleAlert } from "lucide-react";
import { Dialog, Slider, ToggleGroup } from "radix-ui";
import { type PointerEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  addToLibrary,
  type Edits,
  type PieceView,
  type PrepareOptions,
  previewPiece,
  splitToLibrary,
  updatePiece,
} from "./library";
import { formatSize, fromUnit, toUnit, usePreferences } from "./preferences";

type Side = "width" | "height";
type Tool = "brush" | "eraser";

// Green outweighs red for the brush, not for the eraser: the convention `core` reads (`PrepareOptions::edits`).
const TOOL_COLOR: Record<Tool, string> = { brush: "#22c55e", eraser: "#ef4444" };

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

export default function PieceEditor({ image, type, saved, onClose, onSaved }: Props) {
  const { t } = useTranslation("library");
  // The editor is modal, so the unit can't change under the typed size.
  const { unit } = usePreferences();
  const [tolerance, setTolerance] = useState(saved?.options.tolerance ?? 32);
  // The side the jeweller typed, in the display unit; the other one follows the proportions. Until
  // then, `core` takes the size from the file's DPI.
  const [size, setSize] = useState<{ side: Side; text: string } | undefined>(() => {
    const realSize = saved?.options.size;
    if (!realSize) return undefined;
    // Shown like the computed side; a split piece's width is a long fraction.
    const text = (mm: number) => String(Number(toUnit(mm, unit).toFixed(2)));
    return "width" in realSize
      ? { side: "width", text: text(realSize.width) }
      : { side: "height", text: text(realSize.height) };
  });
  const [allowRotation, setAllowRotation] = useState(saved?.options.allowRotation ?? true);
  const [allowMirror, setAllowMirror] = useState(saved?.options.allowMirror ?? false);
  const [preview, setPreview] = useState<PieceView>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [sourceUrl, setSourceUrl] = useState<string>();
  const [tool, setTool] = useState<Tool>();
  const [brushSize, setBrushSize] = useState(20);
  const [edits, setEdits] = useState(saved?.edits);
  // Painted in source pixels; it outlives tolerance and size changes for the whole session.
  const canvas = useRef<HTMLCanvasElement>(null);
  const lastPoint = useRef<DOMPoint>(undefined);
  const painted = useRef(false);

  useEffect(() => {
    const url = URL.createObjectURL(new Blob([image], { type }));
    setSourceUrl(url);
    return () => URL.revokeObjectURL(url);
  }, [image, type]);

  const side = size?.side;
  const mm = fromUnit(Number(size?.text), unit);
  const options = useMemo<PrepareOptions | undefined>(() => {
    if (side && !(mm > 0)) return undefined;
    const realSize = side === "width" ? { width: mm } : side === "height" ? { height: mm } : null;
    return { tolerance, size: realSize, allowRotation, allowMirror };
  }, [tolerance, side, mm, allowRotation, allowMirror]);
  // Decoded by `core` (SVGs rasterised), so the overlay and the edits share its pixel grid.
  const [sourceW, sourceH] = preview?.sourcePx ?? [];

  // Saved strokes go onto the canvas once it exists, so they stay editable. No painting until
  // then, or the first stroke's export would drop them.
  const savedEdits = useRef(saved?.edits);
  useEffect(() => {
    const ctx = canvas.current?.getContext("2d");
    const png = savedEdits.current;
    if (!ctx || !png) return;
    void createImageBitmap(new Blob([png], { type: "image/png" })).then((bitmap) => {
      if (!savedEdits.current) return;
      ctx.drawImage(bitmap, 0, 0);
      savedEdits.current = undefined;
      painted.current = true;
    });
  }, [sourceW]);

  // Re-prepare live, debounced so dragging the slider doesn't queue up a run per step.
  useEffect(() => {
    if (!options) return;
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
      // Until saved strokes are on the canvas, they are still the edits.
      if (!painted.current || !canvas.current) return resolve(edits);
      canvas.current.toBlob((png) =>
        png
          ? png.arrayBuffer().then((buffer) => resolve(new Uint8Array(buffer)))
          : resolve(undefined),
      );
    });

  const accept = (split: boolean) => {
    if (!options) return;
    setSaving(true);
    // Exported afresh: the last stroke's preview may still be pending.
    void exportEdits()
      .then((edits) =>
        split
          ? splitToLibrary(image, options, edits)
          : (saved
              ? updatePiece(saved.id, image, type, options, edits)
              : addToLibrary(image, type, options, edits)
            ).then((piece) => [piece]),
      )
      .then(onSaved, (e) => {
        setError(String(e));
        setSaving(false);
      });
  };

  // A dot on press, then a segment per move; the silhouette is re-prepared once the stroke ends.
  const paint = (e: PointerEvent<SVGSVGElement>, from?: DOMPoint) => {
    const ctx = canvas.current?.getContext("2d");
    const toSource = e.currentTarget.getScreenCTM()?.inverse();
    if (!tool || !ctx || !toSource || savedEdits.current) return;
    const point = new DOMPoint(e.clientX, e.clientY).matrixTransform(toSource);
    const width = brushSize * toSource.a;
    ctx.strokeStyle = ctx.fillStyle = TOOL_COLOR[tool];
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
    painted.current = true;
  };

  const endStroke = () => {
    if (!lastPoint.current) return;
    lastPoint.current = undefined;
    void exportEdits().then(setEdits);
  };

  const slider = (
    label: string,
    value: number,
    min: number,
    max: number,
    onChange: (value: number) => void,
  ) => (
    <Slider.Root
      min={min}
      max={max}
      value={[value]}
      onValueChange={([value]) => onChange(value)}
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

  const sizeInput = (inputSide: Side, index: 0 | 1) => (
    <label className="flex flex-col gap-1 text-sm">
      {t(inputSide, { unit })}
      <input
        type="number"
        min="0"
        step="0.1"
        value={
          size?.side === inputSide
            ? size.text
            : preview
              ? toUnit(preview.sizeMm[index], unit).toFixed(2)
              : ""
        }
        onChange={(e) => setSize({ side: inputSide, text: e.target.value })}
        className="w-28 rounded-md border border-zinc-300 px-2 py-1"
      />
    </label>
  );

  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed inset-6 flex flex-col gap-4 rounded-lg bg-white p-6 shadow-xl">
          <Dialog.Title className="text-xl font-semibold">{t("editorTitle")}</Dialog.Title>
          <Dialog.Description className="sr-only">{t("editorDescription")}</Dialog.Description>

          <div className="flex min-h-0 flex-1 gap-6">
            <div className="checkerboard relative min-w-0 flex-1 rounded-md">
              {sourceUrl && preview && (
                <>
                  <svg
                    viewBox={`0 0 ${sourceW} ${sourceH}`}
                    className={`absolute inset-4 size-[calc(100%-2rem)] touch-none overflow-visible ${tool ? "cursor-crosshair" : ""}`}
                    role="img"
                    aria-label={t("silhouette")}
                    onPointerDown={(e) => {
                      if (!tool || e.button !== 0) return;
                      e.currentTarget.setPointerCapture(e.pointerId);
                      paint(e);
                    }}
                    onPointerMove={(e) => lastPoint.current && paint(e, lastPoint.current)}
                    onPointerUp={endStroke}
                    onPointerCancel={endStroke}
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
                  {/* Letterboxed like the SVG above, so both share the source pixel grid. */}
                  <canvas
                    ref={canvas}
                    width={sourceW}
                    height={sourceH}
                    className="pointer-events-none absolute inset-4 size-[calc(100%-2rem)] object-contain opacity-40"
                  />
                </>
              )}
            </div>

            <div className="flex w-64 flex-col gap-5">
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
                <span>
                  {t("brushSize")}: {brushSize}
                </span>
                {slider(t("brushSize"), brushSize, 2, 100, setBrushSize)}
                <span className="text-xs text-zinc-500">{t("toolsHint")}</span>
              </div>

              <div className="flex gap-3">
                {sizeInput("width", 0)}
                {sizeInput("height", 1)}
              </div>
              {preview && (
                <p className="text-sm text-zinc-600">
                  {formatSize(t, preview.sizeMm, unit)} ·{" "}
                  {t("sizePx", { w: preview.sizePx[0], h: preview.sizePx[1] })}
                </p>
              )}
              {preview?.lowResolution && (
                <p role="status" className="flex gap-2 text-sm text-amber-700">
                  <TriangleAlert aria-hidden className="size-4 shrink-0" />
                  {t("lowResolution", { ppi: Math.round(preview.ppi) })}
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
                disabled={!options || !!error || saving}
                onClick={() => accept(true)}
                className="rounded-md border border-zinc-300 px-4 py-2 text-sm disabled:opacity-40"
              >
                {t("split", { count: preview.contours.length })}
              </button>
            )}
            <button
              type="button"
              disabled={!options || !preview || !!error || saving}
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
