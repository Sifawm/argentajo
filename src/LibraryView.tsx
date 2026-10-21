import { ImagePlus } from "lucide-react";
import { type DragEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import PieceEditor from "./PieceEditor";
import { listLibrary, type PieceView } from "./library";

const IMAGE_TYPES = ["image/png", "image/jpeg", "image/svg+xml"];

export default function LibraryView() {
  const { t } = useTranslation("library");
  const [pieces, setPieces] = useState<PieceView[]>([]);
  const [error, setError] = useState<string>();
  const [importing, setImporting] = useState<{ bytes: Uint8Array<ArrayBuffer>; type: string }>();
  const fileInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    listLibrary().then(setPieces, (e) => setError(String(e)));
  }, []);

  const open = async (file: File | undefined) => {
    if (file && IMAGE_TYPES.includes(file.type))
      setImporting({ bytes: new Uint8Array(await file.arrayBuffer()), type: file.type });
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    void open(e.dataTransfer.files[0]);
  };

  return (
    <section
      className="flex min-h-0 flex-1 flex-col gap-4"
      onDragOver={(e) => e.preventDefault()}
      onDrop={onDrop}
    >
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-semibold">{t("title")}</h2>
        <button
          type="button"
          onClick={() => fileInput.current?.click()}
          className="inline-flex items-center gap-2 rounded-md bg-zinc-900 px-4 py-2 text-sm text-white"
        >
          <ImagePlus aria-hidden className="size-4" />
          {t("import")}
        </button>
        <input
          ref={fileInput}
          type="file"
          accept={IMAGE_TYPES.join(",")}
          hidden
          onChange={(e) => {
            void open(e.target.files?.[0]);
            e.target.value = "";
          }}
        />
      </div>

      {error && (
        <p role="alert" className="text-sm text-red-700">
          {t("loadFailed", { error })}
        </p>
      )}

      {pieces.length === 0 ? (
        <p className="rounded-lg border-2 border-dashed border-zinc-300 p-12 text-center text-zinc-500">
          {t("empty")}
        </p>
      ) : (
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-4">
          {pieces.map((piece) => (
            <li key={piece.id} className="flex flex-col gap-2 rounded-lg bg-white p-3 shadow-sm">
              <img
                src={piece.image}
                alt=""
                className="checkerboard aspect-square w-full rounded object-contain"
              />
              <span className="text-center text-sm text-zinc-600">
                {t("sizeMm", { w: piece.sizeMm[0], h: piece.sizeMm[1] })}
              </span>
            </li>
          ))}
        </ul>
      )}

      {importing && (
        <PieceEditor
          image={importing.bytes}
          type={importing.type}
          onClose={() => setImporting(undefined)}
          onAdded={(added) => {
            setPieces((pieces) => [...pieces, ...added]);
            setImporting(undefined);
          }}
        />
      )}
    </section>
  );
}
