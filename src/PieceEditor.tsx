import { Dialog, Slider } from "radix-ui";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { addToLibrary, type PieceView, type PrepareOptions, previewPiece } from "./library";

type Side = "width" | "height";

interface Props {
  image: Uint8Array;
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
      previewPiece(image, options).then(
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
  }, [image, options]);

  const accept = () => {
    if (!options) return;
    setSaving(true);
    addToLibrary(image, options).then(onAdded, (e) => {
      setError(String(e));
      setSaving(false);
    });
  };

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
            <div className="checkerboard flex min-w-0 flex-1 items-center justify-center rounded-md p-4">
              {preview && (
                <svg
                  viewBox={`0 0 ${preview.sizeMm[0]} ${preview.sizeMm[1]}`}
                  className="size-full overflow-visible"
                  role="img"
                  aria-label={t("silhouette")}
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
                </svg>
              )}
            </div>

            <div className="flex w-64 flex-col gap-5">
              <label className="flex flex-col gap-2 text-sm">
                <span>
                  {t("tolerance")}: {tolerance}
                </span>
                <Slider.Root
                  min={0}
                  max={255}
                  value={[tolerance]}
                  onValueChange={([value]) => setTolerance(value)}
                  className="relative flex h-5 touch-none items-center"
                >
                  <Slider.Track className="relative h-1 grow rounded-full bg-zinc-200">
                    <Slider.Range className="absolute h-full rounded-full bg-zinc-900" />
                  </Slider.Track>
                  <Slider.Thumb
                    aria-label={t("tolerance")}
                    className="block size-4 rounded-full border border-zinc-400 bg-white"
                  />
                </Slider.Root>
                <span className="text-xs text-zinc-500">{t("toleranceHint")}</span>
              </label>

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
