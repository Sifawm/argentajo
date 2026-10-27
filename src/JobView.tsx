import {
  Bookmark,
  CircleStop,
  FileDown,
  FilePlus,
  FolderOpen,
  Play,
  Plus,
  RefreshCw,
  Save,
  TriangleAlert,
  X,
} from "lucide-react";
import { AlertDialog, ToggleGroup } from "radix-ui";
import { type DragEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  addAnotherSize,
  addPiece,
  changeItemSize,
  DEFAULT_OUTPUT,
  dropMissingItems,
  exportPdf,
  fromJobRequest,
  type Item,
  type JobRequest,
  type JobSettings,
  loadMySheets,
  NEW_SHEET,
  type NestView,
  nestJob,
  openJob,
  type OutputOptions,
  PAPERS,
  ROTATION_STEPS,
  type PaperName,
  saveJob,
  saveMySheets,
  setOutputOptions,
  type Size,
  stopNesting,
  toJobRequest,
} from "./job";
import type { PieceView } from "./library";
import { decoded, sizeLabel } from "./library";
import {
  formatSize,
  fromUnit,
  isPositive,
  isSpacing,
  newJob,
  toUnit,
  usePreferences,
} from "./preferences";

/** What Library thumbnails carry when dragged into the Job. */
const DRAG_TYPE = "application/x-argentajo-piece";

export default function JobView({ pieces }: { pieces: PieceView[] }) {
  const { t } = useTranslation("job");
  const { unit, job: defaults } = usePreferences();
  // Read once: a Job keeps its values when the defaults change.
  const [initial] = useState(() => newJob(defaults));
  const [paper, setPaper] = useState<PaperName>(initial.paper);
  const [customPaper, setCustomPaper] = useState<Size>(initial.customPaper);
  const [landscape, setLandscape] = useState(initial.landscape);
  const [printerMargin, setPrinterMargin] = useState(initial.printerMargin);
  const [sheets, setSheets] = useState<Size[]>(initial.sheets);
  const [mySheets, setMySheets] = useState<Size[]>([]);
  const [separation, setSeparation] = useState(initial.separation);
  const [edgeMargin, setEdgeMargin] = useState(initial.edgeMargin);
  const [timeLimit, setTimeLimit] = useState(initial.timeLimit);
  const [rotationStep, setRotationStep] = useState(initial.rotationStep);
  const [items, setItems] = useState<Item[]>(initial.items);
  // The copies of its pieces the Job file opened last holds, untouched by the Library.
  const [jobPieces, setJobPieces] = useState<PieceView[]>([]);
  // With the request it was calculated for: `placed` and `missing` follow its items.
  const [result, setResult] = useState<{ view: NestView; job: JobRequest }>();
  const [pageUrls, setPageUrls] = useState<string[]>([]);
  const [calculating, setCalculating] = useState(false);
  // When the run in progress started, in `performance.now()` ms, and its time limit.
  const [started, setStarted] = useState<{ at: number; seconds: number }>();
  const [iterations, setIterations] = useState(0);
  const [error, setError] = useState<string>();
  const [output, setOutput] = useState<OutputOptions>(DEFAULT_OUTPUT);
  // Shows the print-at-100 % reminder.
  const [exported, setExported] = useState(false);
  // Counts runs, so a superseded one's late progress is ignored.
  const run = useRef(0);

  const byId = useMemo(
    () => new Map([...pieces, ...jobPieces].map((piece) => [piece.id, piece])),
    [pieces, jobPieces],
  );
  // Pieces deleted from the Library, or Sizes deleted from a Piece, drop out of the Job.
  const jobItems = dropMissingItems(items, [...pieces, ...jobPieces]);

  const job = toJobRequest({
    paper,
    customPaper,
    landscape,
    printerMargin,
    sheets,
    separation,
    edgeMargin,
    timeLimit,
    rotationStep,
    items: jobItems,
  });
  const valid =
    sheets.length > 0 &&
    [...job.paper.sizeMm, ...sheets.flat()].every(isPositive) &&
    [printerMargin, separation, edgeMargin].every(isSpacing) &&
    isPositive(timeLimit) &&
    jobItems.length > 0 &&
    jobItems.every((item) => item.fill || (Number.isInteger(item.quantity) && item.quantity > 0));

  // The settings changed since the preview was calculated. The time limit doesn't change a Layout,
  // but a changed Item Size does because `toJobRequest` carries the Size id.
  const stale =
    result &&
    JSON.stringify(result.job) !== JSON.stringify({ ...job, timeLimitS: result.job.timeLimitS });

  useEffect(() => {
    if (!result) return;
    let stale = false;
    const urls = result.view.pages.map((svg) =>
      URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" })),
    );
    // Shown once decoded, so the pages swap without a blank frame in between.
    void Promise.all(urls.map(decoded)).then(() => {
      if (!stale) setPageUrls(urls);
    });
    // The pages on show stay drawn: an image keeps its pixels once its URL is revoked.
    return () => {
      stale = true;
      urls.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [result]);

  useEffect(() => {
    loadMySheets().then(setMySheets, (e) => setError(String(e)));
  }, []);

  const sameSize = (a: Size, b: Size) => a[0] === b[0] && a[1] === b[1];
  const keepMySheets = (next: Size[]) => {
    setMySheets(next);
    saveMySheets(next).catch((e) => setError(t("mySheetsFailed", { error: String(e) })));
  };

  const add = (id: string) => {
    const piece = byId.get(id);
    if (!piece) return;
    setItems((items) => addPiece(items, piece));
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    const id = e.dataTransfer.getData(DRAG_TYPE);
    if (id) add(id);
  };

  // A new run replaces the one in progress.
  const calculate = (next = job) => {
    const id = ++run.current;
    const show = (view: NestView) => {
      if (id === run.current) setResult({ view, job: next });
    };
    setCalculating(true);
    setStarted({ at: performance.now(), seconds: next.timeLimitS });
    setIterations(0);
    setError(undefined);
    nestJob(next, output, show, (n) => {
      if (id === run.current) setIterations(n);
    })
      .then(show, (e) => {
        if (id === run.current) setError(t("nestFailed", { error: String(e) }));
      })
      .finally(() => {
        if (id === run.current) setCalculating(false);
      });
  };

  /** One more Sheet like the last, nested again at once. */
  const addIdenticalSheet = () => {
    const next = [...sheets, sheets.at(-1)!];
    setSheets(next);
    if (valid) calculate({ ...job, sheetsMm: next });
  };

  /** Takes the Item (Piece at one Size) out of the Job and nests what's left, if anything. */
  const exclude = (id: string, size: string) => {
    setItems((items) => items.filter((item) => !(item.id === id && item.size === size)));
    const next = {
      ...job,
      items: job.items.filter((item) => !(item.id === id && item.size_id === size)),
    };
    if (next.items.length > 0) {
      if (valid) calculate(next);
      return;
    }
    run.current++;
    if (calculating) void stopNesting();
    setCalculating(false);
    setResult(undefined);
  };

  const changeOutput = (next: OutputOptions) => {
    setOutput(next);
    setOutputOptions(next).then(
      (pages) => setResult((result) => result && { ...result, view: { ...result.view, pages } }),
      (e) => setError(String(e)),
    );
  };

  const saveFile = () => saveJob(job).catch((e) => setError(t("saveFailed", { error: String(e) })));

  /** Replaces the Job, dropping the run in progress, if any. */
  const load = (
    settings: JobSettings,
    pieces: PieceView[],
    output: OutputOptions,
    result?: { view: NestView; job: JobRequest },
  ) => {
    run.current++;
    setCalculating(false);
    setError(undefined);
    setPaper(settings.paper);
    setCustomPaper(settings.customPaper);
    setLandscape(settings.landscape);
    setPrinterMargin(settings.printerMargin);
    setSheets(settings.sheets);
    setSeparation(settings.separation);
    setEdgeMargin(settings.edgeMargin);
    setTimeLimit(settings.timeLimit);
    setRotationStep(settings.rotationStep);
    setItems(settings.items);
    setJobPieces(pieces);
    setOutput(output);
    setResult(result);
  };

  const newFile = () => {
    if (calculating) void stopNesting();
    load(newJob(defaults), [], DEFAULT_OUTPUT);
  };

  // Only once the file has opened; opening it stops any run.
  const openFile = () =>
    openJob().then(
      (opened) =>
        opened &&
        load(
          fromJobRequest(opened.job),
          opened.pieces,
          opened.output,
          opened.result ? { view: opened.result, job: opened.job } : undefined,
        ),
      (e) => setError(t("openFailed", { error: String(e) })),
    );

  const exportResult = () =>
    exportPdf().then(setExported, (e) => setError(t("exportFailed", { error: String(e) })));

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

  /** Typed in the display unit, kept in mm. */
  const lengthInput = (label: string, mm: number, onChange: (mm: number) => void) =>
    numberInput(
      t(label, { unit }),
      toUnit(mm, unit),
      (value) => onChange(fromUnit(value, unit)),
      "any",
    );

  const thumbnail = (piece: PieceView, className = "size-12") => (
    <img
      src={piece.image}
      alt=""
      className={`checkerboard shrink-0 rounded object-contain ${className}`}
    />
  );

  const changeSize = (item: Item, newSizeId: string) => {
    if (item.size === newSizeId) return;
    setItems((items) =>
      changeItemSize(items, { id: item.id, size: item.size }, newSizeId, [...pieces, ...jobPieces]),
    );
  };

  const counts = result?.job.items.flatMap((item, i) => {
    if (!result) return [];
    const piece = byId.get(item.id);
    const size = piece?.sizes.find((s) => s.id === item.size_id);
    if (!piece || !size) return [];
    const quantity: number = item.quantity === "fill" ? result.view.placed[i] : item.quantity.fixed;
    return [
      {
        piece,
        size,
        label: sizeLabel(piece, size.id, unit, t),
        quantity,
        fill: item.quantity === "fill",
        placed: result.view.placed[i],
        missing: result.view.missing[i],
      },
    ];
  });
  const missing = counts?.filter((count) => count.missing > 0);
  const placed = result?.view.placed.reduce((a, b) => a + b, 0) ?? 0;
  const ordered = counts?.reduce((total, count) => total + count.quantity, 0) ?? 0;

  return (
    <section className="flex min-h-0 flex-1 gap-6">
      <div className="flex w-80 shrink-0 flex-col">
        <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto pr-2">
          <div className="flex gap-2">
            <button
              type="button"
              onClick={newFile}
              className="inline-flex items-center gap-1 rounded-md border border-zinc-300 px-2 py-1 text-sm"
            >
              <FilePlus aria-hidden className="size-4" />
              {t("newJob")}
            </button>
            <button
              type="button"
              onClick={() => void openFile()}
              className="inline-flex items-center gap-1 rounded-md border border-zinc-300 px-2 py-1 text-sm"
            >
              <FolderOpen aria-hidden className="size-4" />
              {t("openJob")}
            </button>
            <button
              type="button"
              disabled={!valid || calculating}
              onClick={() => void saveFile()}
              className="inline-flex items-center gap-1 rounded-md border border-zinc-300 px-2 py-1 text-sm disabled:opacity-40"
            >
              <Save aria-hidden className="size-4" />
              {t("saveJob")}
            </button>
          </div>
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
                {lengthInput("width", customPaper[0], (w) => setCustomPaper([w, customPaper[1]]))}
                {lengthInput("height", customPaper[1], (h) => setCustomPaper([customPaper[0], h]))}
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
            {lengthInput("printerMargin", printerMargin, setPrinterMargin)}
          </Group>

          <Group title={t("sheets")}>
            <ol className="flex flex-col gap-2">
              {sheets.map((sheet, i) => {
                const update = (size: Size) =>
                  setSheets((sheets) => sheets.map((s, j) => (j === i ? size : s)));
                return (
                  <li key={i} className="flex items-end gap-2">
                    {lengthInput("width", sheet[0], (w) => update([w, sheet[1]]))}
                    {lengthInput("height", sheet[1], (h) => update([sheet[0], h]))}
                    <button
                      type="button"
                      aria-label={t("saveSheet")}
                      title={t("saveSheet")}
                      disabled={
                        !sheet.every(isPositive) || mySheets.some((s) => sameSize(s, sheet))
                      }
                      onClick={() => keepMySheets([...mySheets, sheet])}
                      className="rounded-md p-1 text-zinc-500 hover:text-zinc-900 disabled:opacity-40"
                    >
                      <Bookmark aria-hidden className="size-4" />
                    </button>
                    <button
                      type="button"
                      aria-label={t("removeSheet")}
                      title={t("removeSheet")}
                      onClick={() => setSheets((sheets) => sheets.filter((_, j) => j !== i))}
                      className="rounded-md p-1 text-zinc-500 hover:text-red-700"
                    >
                      <X aria-hidden className="size-4" />
                    </button>
                  </li>
                );
              })}
            </ol>
            <button
              type="button"
              onClick={() => setSheets((sheets) => [...sheets, sheets.at(-1) ?? NEW_SHEET])}
              className="inline-flex items-center gap-1 self-start rounded-md border border-zinc-300 px-2 py-1 text-sm"
            >
              <Plus aria-hidden className="size-4" />
              {t("addSheet")}
            </button>
            {mySheets.length > 0 && (
              <div className="flex flex-col gap-1">
                <span className="text-xs text-zinc-500">{t("mySheets")}</span>
                <ul className="flex flex-wrap gap-2">
                  {mySheets.map((size) => {
                    const label = formatSize(t, size, unit);
                    return (
                      <li
                        key={size.join("x")}
                        className="flex items-center rounded-md border border-zinc-300 text-xs"
                      >
                        <button
                          type="button"
                          onClick={() => setSheets((sheets) => [...sheets, size])}
                          className="py-1 pr-1 pl-2"
                        >
                          {label}
                        </button>
                        <button
                          type="button"
                          aria-label={t("forgetSheet", { size: label })}
                          title={t("forgetSheet", { size: label })}
                          onClick={() => keepMySheets(mySheets.filter((s) => !sameSize(s, size)))}
                          className="p-1 text-zinc-500 hover:text-red-700"
                        >
                          <X aria-hidden className="size-3" />
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </div>
            )}
            <div className="flex gap-3">
              {lengthInput("separation", separation, setSeparation)}
              {lengthInput("edgeMargin", edgeMargin, setEdgeMargin)}
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
                  <li
                    key={`${item.id}:${item.size}`}
                    className="flex items-start gap-3 rounded-md border border-zinc-200 p-2"
                  >
                    {thumbnail(piece)}
                    <div className="flex min-w-0 flex-1 flex-col gap-2">
                      <select
                        value={item.size}
                        disabled={item.fixed}
                        onChange={(e) => changeSize(item, e.target.value)}
                        className="min-w-0 rounded-md border border-zinc-300 px-2 py-1 text-xs disabled:opacity-60"
                      >
                        {piece.sizes.map((size) => (
                          <option key={size.id} value={size.id}>
                            {sizeLabel(piece, size.id, unit, t)}
                          </option>
                        ))}
                      </select>
                      <div className="flex items-center gap-3">
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
                              items.map((i) =>
                                i.id === item.id && i.size === item.size ? { ...i, quantity } : i,
                              ),
                            );
                          }}
                          className="w-20 rounded-md border border-zinc-300 px-2 py-1 text-sm disabled:opacity-40"
                        />
                        <label className="flex items-center gap-1 text-xs" title={t("fillHint")}>
                          <input
                            type="checkbox"
                            checked={item.fill}
                            onChange={(e) => {
                              const fill = e.target.checked;
                              setItems((items) =>
                                items.map((i) =>
                                  i.id === item.id && i.size === item.size ? { ...i, fill } : i,
                                ),
                              );
                            }}
                          />
                          {t("fill")}
                        </label>
                      </div>
                    </div>
                    <div className="flex flex-col items-center gap-1">
                      <button
                        type="button"
                        aria-label={t("remove")}
                        title={t("remove")}
                        onClick={() =>
                          setItems((items) =>
                            items.filter((i) => !(i.id === item.id && i.size === item.size)),
                          )
                        }
                        className="rounded-md p-1 text-zinc-500 hover:text-red-700"
                      >
                        <X aria-hidden className="size-4" />
                      </button>
                      <button
                        type="button"
                        disabled={item.fixed || !addAnotherSize(items, piece)}
                        onClick={() => {
                          const next = addAnotherSize(items, piece);
                          if (next) setItems(next);
                        }}
                        aria-label={t("addAnotherSize")}
                        title={t("addAnotherSize")}
                        className="rounded-md p-1 text-zinc-500 hover:text-zinc-900 disabled:opacity-40"
                      >
                        <Plus aria-hidden className="size-4" />
                      </button>
                    </div>
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
                    aria-label={t("addPiece", {
                      size: sizeLabel(piece, piece.sizes[0]?.id ?? "", unit, t),
                    })}
                    title={sizeLabel(piece, piece.sizes[0]?.id ?? "", unit, t)}
                    className="w-full rounded bg-white p-1 shadow-sm"
                  >
                    {thumbnail(piece, "aspect-square w-full")}
                  </button>
                </li>
              ))}
            </ul>
          </Group>
        </div>
        {/* Outside the scroll area: a sticky footer would hide what Tab focuses underneath it. */}
        <div className="flex flex-col gap-3 border-t border-zinc-200 py-3 pr-2">
          <div className="flex gap-3">
            {numberInput(t("timeLimit"), timeLimit, setTimeLimit, "1")}
            <label className="flex flex-col gap-1 text-sm">
              {t("rotationStep")}
              <select
                value={rotationStep}
                onChange={(e) => setRotationStep(Number(e.target.value))}
                className="rounded-md border border-zinc-300 px-2 py-1"
              >
                {ROTATION_STEPS.map((step) => (
                  <option key={step} value={step}>
                    {step}°
                  </option>
                ))}
              </select>
            </label>
          </div>
          {calculating ? (
            <>
              <button
                type="button"
                onClick={() => void stopNesting()}
                className="inline-flex items-center justify-center gap-2 rounded-md bg-zinc-900 px-4 py-2 text-sm text-white"
              >
                <CircleStop aria-hidden className="size-4" />
                {t("stop")}
              </button>
              {started && <TimeLeft key={started.at} {...started} iterations={iterations} />}
            </>
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
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-3">
        {result ? (
          <>
            <div className="flex flex-wrap items-center gap-4 text-sm">
              <label className="flex items-center gap-1">
                <input
                  type="checkbox"
                  checked={output.sheetOutline}
                  onChange={(e) => changeOutput({ ...output, sheetOutline: e.target.checked })}
                />
                {t("sheetOutline")}
              </label>
              <label className="flex items-center gap-1">
                <input
                  type="checkbox"
                  checked={output.silhouette}
                  onChange={(e) => changeOutput({ ...output, silhouette: e.target.checked })}
                />
                {t("silhouette")}
              </label>
              <button
                type="button"
                disabled={calculating || stale}
                onClick={() => void exportResult()}
                className="inline-flex items-center gap-2 rounded-md bg-zinc-900 px-3 py-1 text-white disabled:opacity-40"
              >
                <FileDown aria-hidden className="size-4" />
                {t("exportPdf")}
              </button>
            </div>
            <p className="text-sm" role="status">
              {t("utilisation", { value: result.view.utilisation })} ·{" "}
              {t("placed", { placed, count: ordered })}
            </p>
            <ul className="flex flex-wrap gap-4 text-sm">
              {counts?.map((count) => (
                <li key={`${count.piece.id}:${count.size.id}`} className="flex items-center gap-2">
                  {thumbnail(count.piece, "size-8")}
                  <span className="text-xs text-zinc-600">{count.label}</span>
                  {count.fill
                    ? t("placedFill", { placed: count.placed })
                    : t("placedOf", { placed: count.placed, quantity: count.quantity })}
                  <button
                    type="button"
                    aria-label={t("exclude")}
                    title={t("exclude")}
                    onClick={() => exclude(count.piece.id!, count.size.id)}
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
                  {missing.map(({ piece, size, missing, label }) => (
                    <li key={`${piece.id}:${size.id}`} className="flex items-center gap-2">
                      {t("times", { count: missing })}
                      {thumbnail(piece, "size-8")}
                      <span className="text-xs">{label}</span>
                    </li>
                  ))}
                </ul>
                {!calculating && !stale && sheets.length > 0 && (
                  <button
                    type="button"
                    onClick={addIdenticalSheet}
                    className="inline-flex items-center gap-1 self-start rounded-md border border-amber-700 px-2 py-1"
                  >
                    <Plus aria-hidden className="size-4" />
                    {t("addIdenticalSheet")}
                  </button>
                )}
              </div>
            )}
            <div
              className={`flex min-h-0 flex-1 gap-4 overflow-x-auto rounded-md bg-zinc-200 p-4 ${stale ? "opacity-50" : ""}`}
            >
              {pageUrls.map((url, i) => (
                <img
                  // By page, not URL: a new element would start out blank.
                  key={i}
                  src={url}
                  alt={t("preview", { page: i + 1 })}
                  className="h-full max-w-full shrink-0 object-contain"
                />
              ))}
            </div>
          </>
        ) : (
          <p className="m-auto text-zinc-500">{t("noResult")}</p>
        )}
      </div>

      <AlertDialog.Root open={exported} onOpenChange={setExported}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="fixed inset-0 bg-black/40" />
          <AlertDialog.Content className="fixed top-1/2 left-1/2 flex w-96 -translate-1/2 flex-col gap-4 rounded-lg bg-white p-6 shadow-xl">
            <AlertDialog.Title className="text-lg font-semibold">{t("exported")}</AlertDialog.Title>
            <AlertDialog.Description className="text-sm text-zinc-600">
              {t("printAtActualSize")}
            </AlertDialog.Description>
            <AlertDialog.Action className="self-end rounded-md bg-zinc-900 px-4 py-2 text-sm text-white">
              {t("ok")}
            </AlertDialog.Action>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </section>
  );
}

/**
 * Counts down the run's time limit, with the iterations tried so far. The first Layout is always
 * waited for, so it may run over.
 */
function TimeLeft({
  at,
  seconds,
  iterations,
}: {
  at: number;
  seconds: number;
  iterations: number;
}) {
  const { t } = useTranslation("job");
  const [now, setNow] = useState(at);
  useEffect(() => {
    const id = setInterval(() => setNow(performance.now()), 250);
    return () => clearInterval(id);
  }, []);
  const elapsed = Math.min((now - at) / 1000, seconds);
  const left = Math.ceil(seconds - elapsed);
  return (
    <label className="flex flex-col gap-1 text-xs text-zinc-600">
      <span>
        {left > 0 ? t("timeLeft", { count: left }) : t("finishing")} ·{" "}
        {t("iterations", { count: iterations })}
      </span>
      <progress max={seconds} value={elapsed} className="w-full" />
    </label>
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
