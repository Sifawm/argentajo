import { Brush, Eraser } from "lucide-react";
import { Dialog, Slider, ToggleGroup } from "radix-ui";
import { type PointerEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  addToLibrary,
  type Edits,
  type PieceView,
  type PrepareOptions,
  previewPiece,
} from "./library";

type Side = "width" | "height";
type Tool = "brush" | "eraser";

// Green outweighs red for the brush, not for the eraser: the convention `core` reads (`PrepareOptions::edits`).
const TOOL_COLOR: Record<Tool, string> = { brush: "#22c55e", eraser: "#ef4444" };

interface Props {
  image: Uint8Array<ArrayBuffer>;
  onClose: () => void;
  onAdded: (piece: PieceView) => void;
}

export default function PieceEditor({ image, onClose, onAdded }: Props) {
  const { t } = useTranslation("library");
  const [tolerance, setTolerance] = useState(32);
  // The side the jeweller typed; the other one follows the proportions.
  const [size, setSize] = useState<{ side: Side; text: string }>({ side: "width", text: "20" });
  const [allowRotation, setAllowRotation] = useState(true);
  const [allowMirror, setAllowMirror] = useState(false);
  const [preview, setPreview] = useState<PieceView>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [source, setSource] = useState<{ url: string; w: number; h: number }>();
  const [tool, setTool] = useState<Tool>();
  const [brushSize, setBrushSize] = useState(20);
  const [edits, setEdits] = useState<Edits>();
  // Painted in source pixels; it outlives tolerance and size changes for the whole session.
  const canvas = useRef<HTMLCanvasElement>(null);
  const lastPoint = useRef<DOMPoint>(undefined);
  const painted = useRef(false);

  useEffect(() => {
    let stale = false;
    const url = URL.createObjectURL(new Blob([image]));
    const img = new Image();
    img.src = url;
    img.decode().then(
      () => !stale && setSource({ url, w: img.naturalWidth, h: img.naturalHeight }),
      (e) => !stale && setError(String(e)),
    );
    return () => {
      stale = true;
      URL.revokeObjectURL(url);
    };
  }, [image]);

  const mm = Number(size.text);
  const options = useMemo<PrepareOptions | undefined>(
    () =>
      mm > 0
        ? {
            tolerance,
            size: size.side === "width" ? { width: mm } : { height: mm },
            allowRotation,
            allowMirror,
          }
        : undefined,
    [tolerance, size.side, mm, allowRotation, allowMirror],
  );

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
      if (!painted.current || !canvas.current) return resolve(undefined);
      canvas.current.toBlob((png) =>
        png
          ? png.arrayBuffer().then((buffer) => resolve(new Uint8Array(buffer)))
          : resolve(undefined),
      );
    });

  const accept = () => {
    if (!options) return;
    setSaving(true);
    // Exported afresh: the last stroke's preview may still be pending.
    void exportEdits()
      .then((edits) => addToLibrary(image, options, edits))
      .then(onAdded, (e) => {
        setError(String(e));
        setSaving(false);
      });
  };

  // A dot on press, then a segment per move; the silhouette is re-prepared once the stroke ends.
  const paint = (e: PointerEvent<SVGSVGElement>, from?: DOMPoint) => {
    const ctx = canvas.current?.getContext("2d");
    const toSource = e.currentTarget.getScreenCTM()?.inverse();
    if (!tool || !ctx || !toSource) return;
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

  const sizeInput = (side: Side, index: 0 | 1) => (
    <label className="flex flex-col gap-1 text-sm">
      {t(side)}
      <input
        type="number"
        min="0"
        step="0.1"
        value={size.side === side ? size.text : (preview?.sizeMm[index].toFixed(2) ?? "")}
        onChange={(e) => setSize({ side, text: e.target.value })}
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
              {source && (
                <>
                  <svg
                    viewBox={`0 0 ${source.w} ${source.h}`}
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
                    <image href={source.url} width={source.w} height={source.h} opacity={0.25} />
                    {preview && (
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
                    )}
                  </svg>
                  {/* Letterboxed like the SVG above, so both share the source pixel grid. */}
                  <canvas
                    ref={canvas}
                    width={source.w}
                    height={source.h}
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
                  {t("sizeMm", { w: preview.sizeMm[0], h: preview.sizeMm[1] })} ·{" "}
                  {t("sizePx", { w: preview.sizePx[0], h: preview.sizePx[1] })}
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
            <button
              type="button"
              disabled={!options || !preview || !!error || saving}
              onClick={accept}
              className="rounded-md bg-zinc-900 px-4 py-2 text-sm text-white disabled:opacity-40"
            >
              {t("accept")}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
