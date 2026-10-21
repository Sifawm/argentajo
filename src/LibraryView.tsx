import { ImagePlus, Trash2 } from "lucide-react";
import { AlertDialog } from "radix-ui";
import { type ComponentProps, type DragEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import PieceEditor from "./PieceEditor";
import { deletePiece, listLibrary, openPiece, type PieceView } from "./library";

const IMAGE_TYPES = ["image/png", "image/jpeg", "image/svg+xml"];

export default function LibraryView() {
  const { t } = useTranslation("library");
  const [pieces, setPieces] = useState<PieceView[]>([]);
  const [error, setError] = useState<string>();
  const [editing, setEditing] =
    useState<Pick<ComponentProps<typeof PieceEditor>, "image" | "type" | "saved">>();
  const [deleting, setDeleting] = useState<PieceView>();
  const fileInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    listLibrary().then(setPieces, (e) => setError(t("loadFailed", { error: String(e) })));
  }, []);

  const open = async (file: File | undefined) => {
    if (file && IMAGE_TYPES.includes(file.type))
      setEditing({ image: new Uint8Array(await file.arrayBuffer()), type: file.type });
  };

  const edit = (id: string) =>
    openPiece(id).then(
      ({ image, type, options, edits }) =>
        setEditing({ image, type, saved: { id, options, edits } }),
      (e) => setError(t("openFailed", { error: String(e) })),
    );

  const remove = (id: string) =>
    deletePiece(id).then(
      () => setPieces((pieces) => pieces.filter((piece) => piece.id !== id)),
      (e) => setError(t("deleteFailed", { error: String(e) })),
    );

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
          {error}
        </p>
      )}

      {pieces.length === 0 ? (
        <p className="rounded-lg border-2 border-dashed border-zinc-300 p-12 text-center text-zinc-500">
          {t("empty")}
        </p>
      ) : (
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-4">
          {pieces.map((piece) => (
            <li
              key={piece.id}
              className="group relative flex flex-col gap-2 rounded-lg bg-white p-3 shadow-sm"
            >
              <button
                type="button"
                aria-label={t("edit")}
                title={t("edit")}
                onDoubleClick={() => void edit(piece.id!)}
                onKeyDown={(e) =>
                  (e.key === "Enter" || e.key === " ") && !e.repeat && void edit(piece.id!)
                }
                className="rounded"
              >
                <img
                  src={piece.image}
                  alt=""
                  className="checkerboard aspect-square w-full rounded object-contain"
                />
              </button>
              <button
                type="button"
                aria-label={t("delete")}
                title={t("delete")}
                onClick={() => setDeleting(piece)}
                className="absolute top-4 right-4 rounded-md bg-white/90 p-1.5 text-zinc-600 opacity-0 shadow-sm group-hover:opacity-100 hover:text-red-700 focus-visible:opacity-100"
              >
                <Trash2 aria-hidden className="size-4" />
              </button>
              <span className="text-center text-sm text-zinc-600">
                {t("sizeMm", { w: piece.sizeMm[0], h: piece.sizeMm[1] })}
              </span>
            </li>
          ))}
        </ul>
      )}

      {editing && (
        <PieceEditor
          {...editing}
          onClose={() => setEditing(undefined)}
          onSaved={(saved) => {
            setPieces((pieces) => [
              ...pieces.map((piece) => saved.find((s) => s.id === piece.id) ?? piece),
              ...saved.filter((s) => !pieces.some((piece) => piece.id === s.id)),
            ]);
            setEditing(undefined);
          }}
        />
      )}

      <AlertDialog.Root open={!!deleting} onOpenChange={(open) => !open && setDeleting(undefined)}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="fixed inset-0 bg-black/40" />
          <AlertDialog.Content className="fixed top-1/2 left-1/2 flex w-96 -translate-1/2 flex-col gap-4 rounded-lg bg-white p-6 shadow-xl">
            <AlertDialog.Title className="text-lg font-semibold">
              {t("deleteTitle")}
            </AlertDialog.Title>
            {deleting && (
              <img
                src={deleting.image}
                alt=""
                className="checkerboard mx-auto size-24 rounded object-contain"
              />
            )}
            <AlertDialog.Description className="text-sm text-zinc-600">
              {t("deleteDescription")}
            </AlertDialog.Description>
            <div className="flex justify-end gap-3">
              <AlertDialog.Cancel className="rounded-md px-4 py-2 text-sm hover:bg-zinc-100">
                {t("cancel")}
              </AlertDialog.Cancel>
              <AlertDialog.Action
                onClick={() => deleting && void remove(deleting.id!)}
                className="rounded-md bg-red-700 px-4 py-2 text-sm text-white"
              >
                {t("delete")}
              </AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </section>
  );
}
