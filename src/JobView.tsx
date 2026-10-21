import { CircleStop, LoaderCircle, Play, RefreshCw, TriangleAlert, X } from "lucide-react";
import { ToggleGroup } from "radix-ui";
import { type DragEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type JobRequest, type NestView, nestJob, PAPERS, stopNesting } from "./job";
import type { PieceView } from "./library";

type PaperName = keyof typeof PAPERS | "custom";
/** The quantity is kept while the item is on fill, for when it's switched back. */
type Item = { id: string; quantity: number; fill: boolean };

/** What Library thumbnails carry when dragged into the Job. */
const DRAG_TYPE = "application/x-argentajo-piece";

const isPositive = (n: number) => Number.isFinite(n) && n > 0;
const isSpacing = (n: number) => Number.isFinite(n) && n >= 0;

export default function JobView({ pieces }: { pieces: PieceView[] }) {
  const { t } = useTranslation("job");
  const [paper, setPaper] = useState<PaperName>("a4");
  const [customPaper, setCustomPaper] = useState<[number, number]>([210, 297]);
  const [landscape, setLandscape] = useState(false);
  const [printerMargin, setPrinterMargin] = useState(5);
  const [sheet, setSheet] = useState<[number, number]>([100, 50]);
  const [separation, setSeparation] = useState(1);
  const [edgeMargin, setEdgeMargin] = useState(1.5);
  const [timeLimit, setTimeLimit] = useState(60);
  const [items, setItems] = useState<Item[]>([]);
  // With the request it was calculated for: `placed` and `missing` follow its items.
  const [result, setResult] = useState<{ view: NestView; job: JobRequest }>();
  const [svgUrl, setSvgUrl] = useState<string>();
  const [calculating, setCalculating] = useState(false);
  const [error, setError] = useState<string>();
  // Counts runs, so a superseded one's late progress is ignored.
  const run = useRef(0);

  const byId = useMemo(() => new Map(pieces.map((piece) => [piece.id, piece])), [pieces]);
  // Pieces deleted from the Library drop out of the Job.
  const jobItems = items.filter((item) => byId.has(item.id));

  const [shortSide, longSide] = [...(paper === "custom" ? customPaper : PAPERS[paper])].sort(
    (a, b) => a - b,
  );
  const job: JobRequest = {
    paper: {
      sizeMm: landscape ? [longSide, shortSide] : [shortSide, longSide],
      marginMm: printerMargin,
    },
    sheetMm: sheet,
    separationMm: separation,
    edgeMarginMm: edgeMargin,
    timeLimitS: timeLimit,
    items: jobItems.map(({ id, quantity, fill }) => ({
      id,
      quantity: fill ? "fill" : { fixed: quantity },
    })),
  };
  const valid =
    [...job.paper.sizeMm, ...sheet].every(isPositive) &&
    [printerMargin, separation, edgeMargin].every(isSpacing) &&
    isPositive(timeLimit) &&
    jobItems.length > 0 &&
    jobItems.every((item) => item.fill || (Number.isInteger(item.quantity) && item.quantity > 0));

  // The settings changed since the preview was calculated. The time limit doesn't change a Layout.
  const stale =
    result &&
    JSON.stringify(result.job) !== JSON.stringify({ ...job, timeLimitS: result.job.timeLimitS });

  useEffect(() => {
    if (!result) return;
    const url = URL.createObjectURL(new Blob([result.view.svg], { type: "image/svg+xml" }));
    setSvgUrl(url);
    return () => URL.revokeObjectURL(url);
  }, [result]);

  const add = (id: string) =>
    setItems((items) =>
      items.some((item) => item.id === id)
        ? items.map((item) => (item.id === id ? { ...item, quantity: item.quantity + 1 } : item))
        : [...items, { id, quantity: 1, fill: false }],
    );

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    const id = e.dataTransfer.getData(DRAG_TYPE);
    if (id) add(id);
  };

  // A new run, with a new seed, replaces the one in progress.
  const calculate = (next = job) => {
    const id = ++run.current;
    const show = (view: NestView) => {
      if (id === run.current) setResult({ view, job: next });
    };
    setCalculating(true);
    setError(undefined);
    nestJob(next, show)
      .then(show, (e) => {
        if (id === run.current) setError(t("nestFailed", { error: String(e) }));
      })
      .finally(() => {
        if (id === run.current) setCalculating(false);
      });
  };

  /** Takes the piece out of the Job and nests what's left, if anything. */
  const exclude = (id: string) => {
    setItems((items) => items.filter((item) => item.id !== id));
    const next = { ...job, items: job.items.filter((item) => item.id !== id) };
    if (next.items.length > 0) {
      if (valid) calculate(next);
      return;
    }
    run.current++;
    if (calculating) void stopNesting();
    setCalculating(false);
    setResult(undefined);
  };

  const numberInput = (
    label: string,
    value: number,
    onChange: (value: number) => void,
    step = "0.1",
  ) => (
    <label className="flex flex-col gap-1 text-sm">
      {label}
      <input
        type="number"
        min="0"
        step={step}
        value={Number.isNaN(value) ? "" : value}
        onChange={(e) => onChange(e.target.valueAsNumber)}
        className="w-24 rounded-md border border-zinc-300 px-2 py-1"
      />
    </label>
  );

  const thumbnail = (piece: PieceView, className = "size-12") => (
    <img
      src={piece.image}
      alt=""
      className={`checkerboard shrink-0 rounded object-contain ${className}`}
    />
  );

  const sizeMm = (piece: PieceView) =>
    t("library:sizeMm", { w: piece.sizeMm[0], h: piece.sizeMm[1] });

  const counts = result?.job.items.flatMap((item, i) => {
    const piece = byId.get(item.id);
    const fill = item.quantity === "fill";
    return piece
      ? [
          {
            piece,
            // Fill asks for as many as were placed.
            quantity: item.quantity === "fill" ? result.view.placed[i] : item.quantity.fixed,
            fill,
            placed: result.view.placed[i],
            missing: result.view.missing[i],
          },
        ]
      : [];
  });
  const missing = counts?.filter((count) => count.missing > 0);
  const placed = result?.view.placed.reduce((a, b) => a + b, 0) ?? 0;
  const ordered = counts?.reduce((total, count) => total + count.quantity, 0) ?? 0;

  return (
    <section className="flex min-h-0 flex-1 gap-6">
      <div className="flex w-80 shrink-0 flex-col gap-5 overflow-y-auto pr-2">
        <Group title={t("paper")}>
          <label className="flex flex-col gap-1 text-sm">
            {t("paperSize")}
            <select
              value={paper}
              onChange={(e) => setPaper(e.target.value as PaperName)}
              className="rounded-md border border-zinc-300 px-2 py-1"
            >
              {Object.keys(PAPERS).map((name) => (
                <option key={name} value={name}>
                  {t(`papers.${name}`)}
                </option>
              ))}
              <option value="custom">{t("papers.custom")}</option>
            </select>
          </label>
          {paper === "custom" && (
            <div className="flex gap-3">
              {numberInput(t("widthMm"), customPaper[0], (w) =>
                setCustomPaper([w, customPaper[1]]),
              )}
              {numberInput(t("heightMm"), customPaper[1], (h) =>
                setCustomPaper([customPaper[0], h]),
              )}
            </div>
          )}
          <ToggleGroup.Root
            type="single"
            aria-label={t("orientation")}
            value={landscape ? "landscape" : "portrait"}
            onValueChange={(value) => value && setLandscape(value === "landscape")}
            className="inline-flex self-start rounded-md border border-zinc-300"
          >
            {(["portrait", "landscape"] as const).map((value) => (
              <ToggleGroup.Item
                key={value}
                value={value}
                className="px-3 py-1 text-sm first:rounded-l-md last:rounded-r-md data-[state=on]:bg-zinc-900 data-[state=on]:text-white"
              >
                {t(value)}
              </ToggleGroup.Item>
            ))}
          </ToggleGroup.Root>
          {numberInput(t("printerMargin"), printerMargin, setPrinterMargin)}
        </Group>

        <Group title={t("sheet")}>
          <div className="flex gap-3">
            {numberInput(t("widthMm"), sheet[0], (w) => setSheet([w, sheet[1]]))}
            {numberInput(t("heightMm"), sheet[1], (h) => setSheet([sheet[0], h]))}
          </div>
          <div className="flex gap-3">
            {numberInput(t("separation"), separation, setSeparation)}
            {numberInput(t("edgeMargin"), edgeMargin, setEdgeMargin)}
          </div>
        </Group>

        <Group title={t("pieces")}>
          <ul
            onDragOver={(e) => e.preventDefault()}
            onDrop={onDrop}
            className="flex min-h-20 flex-col gap-2 rounded-md border-2 border-dashed border-zinc-300 p-2"
          >
            {jobItems.length === 0 && (
              <li className="m-auto text-center text-sm text-zinc-500">{t("dropHere")}</li>
            )}
            {jobItems.map((item) => {
              const piece = byId.get(item.id)!;
              return (
                <li key={item.id} className="flex items-center gap-3">
                  {thumbnail(piece)}
                  <span className="flex-1 text-xs text-zinc-600">{sizeMm(piece)}</span>
                  <input
                    type="number"
                    min="1"
                    step="1"
                    aria-label={t("quantity")}
                    disabled={item.fill}
                    value={Number.isNaN(item.quantity) ? "" : item.quantity}
                    onChange={(e) => {
                      const quantity = e.target.valueAsNumber;
                      setItems((items) =>
                        items.map((i) => (i.id === item.id ? { ...i, quantity } : i)),
                      );
                    }}
                    className="w-16 rounded-md border border-zinc-300 px-2 py-1 text-sm disabled:opacity-40"
                  />
                  <label className="flex items-center gap-1 text-xs" title={t("fillHint")}>
                    <input
                      type="checkbox"
                      checked={item.fill}
                      onChange={(e) => {
                        const fill = e.target.checked;
                        setItems((items) =>
                          items.map((i) => (i.id === item.id ? { ...i, fill } : i)),
                        );
                      }}
                    />
                    {t("fill")}
                  </label>
                  <button
                    type="button"
                    aria-label={t("remove")}
                    title={t("remove")}
                    onClick={() => setItems((items) => items.filter((i) => i.id !== item.id))}
                    className="rounded-md p-1 text-zinc-500 hover:text-red-700"
                  >
                    <X aria-hidden className="size-4" />
                  </button>
                </li>
              );
            })}
          </ul>
          <span className="text-xs text-zinc-500">{t("libraryHint")}</span>
          <ul className="grid grid-cols-4 gap-2">
            {pieces.map((piece) => (
              <li key={piece.id}>
                <button
                  type="button"
                  draggable
                  onDragStart={(e) => e.dataTransfer.setData(DRAG_TYPE, piece.id!)}
                  onClick={() => add(piece.id!)}
                  aria-label={t("addPiece", { size: sizeMm(piece) })}
                  title={sizeMm(piece)}
                  className="w-full rounded bg-white p-1 shadow-sm"
                >
                  {thumbnail(piece, "aspect-square w-full")}
                </button>
              </li>
            ))}
          </ul>
        </Group>

        {numberInput(t("timeLimit"), timeLimit, setTimeLimit, "1")}
        {calculating ? (
          <button
            type="button"
            onClick={() => void stopNesting()}
            className="inline-flex items-center justify-center gap-2 rounded-md bg-zinc-900 px-4 py-2 text-sm text-white"
          >
            <CircleStop aria-hidden className="size-4" />
            {t("stop")}
          </button>
        ) : (
          <button
            type="button"
            disabled={!valid}
            onClick={() => calculate()}
            className="inline-flex items-center justify-center gap-2 rounded-md bg-zinc-900 px-4 py-2 text-sm text-white disabled:opacity-40"
          >
            {result && !stale ? (
              <RefreshCw aria-hidden className="size-4" />
            ) : (
              <Play aria-hidden className="size-4" />
            )}
            {t(result && !stale ? "recalculate" : "calculate")}
          </button>
        )}
        {error && (
          <p role="alert" className="text-sm text-red-700">
            {error}
          </p>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-3">
        {result ? (
          <>
            <p className="flex items-center gap-2 text-sm" role="status">
              {calculating && <LoaderCircle aria-hidden className="size-4 animate-spin" />}
              {t("utilisation", { value: result.view.utilisation })} ·{" "}
              {t("placed", { placed, count: ordered })}
            </p>
            <ul className="flex flex-wrap gap-4 text-sm">
              {counts?.map((count) => (
                <li key={count.piece.id} className="flex items-center gap-2">
                  {thumbnail(count.piece, "size-8")}
                  {count.fill
                    ? t("placedFill", { placed: count.placed })
                    : t("placedOf", { placed: count.placed, quantity: count.quantity })}
                  <button
                    type="button"
                    aria-label={t("exclude")}
                    title={t("exclude")}
                    onClick={() => exclude(count.piece.id!)}
                    className="rounded-md p-1 text-zinc-500 hover:text-red-700"
                  >
                    <X aria-hidden className="size-4" />
                  </button>
                </li>
              ))}
            </ul>
            {stale && <p className="text-sm text-zinc-500">{t("stale")}</p>}
            {missing && missing.length > 0 && (
              <div role="alert" className="flex flex-col gap-2 text-sm text-amber-700">
                <span className="flex items-center gap-2">
                  <TriangleAlert aria-hidden className="size-4 shrink-0" />
                  {t("missing")}
                </span>
                <ul className="flex flex-wrap gap-4">
                  {missing.map(({ piece, missing }) => (
                    <li key={piece.id} className="flex items-center gap-2">
                      {t("times", { count: missing })}
                      {thumbnail(piece, "size-8")}
                      <span className="text-xs">{sizeMm(piece)}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}
            <img
              src={svgUrl}
              alt={t("preview")}
              className={`min-h-0 w-full flex-1 rounded-md bg-zinc-200 object-contain p-4 ${stale ? "opacity-50" : ""}`}
            />
          </>
        ) : (
          <p className="m-auto text-zinc-500">{t("noResult")}</p>
        )}
      </div>
    </section>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className="flex flex-col gap-3">
      <legend className="mb-2 font-semibold">{title}</legend>
      {children}
    </fieldset>
  );
}
