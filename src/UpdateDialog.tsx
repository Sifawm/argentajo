import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Dialog } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

/** Looks for a newer release once at startup and, if there is one, offers to install it. */
export default function UpdateDialog() {
  const { t } = useTranslation();
  const [update, setUpdate] = useState<Update>();
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    // Offline, or a .deb that can't update itself: say nothing, the next start looks again.
    invoke<boolean>("updates_supported")
      .then((supported) => (supported ? check() : null))
      .then(
        (found) => found && setUpdate(found),
        () => {},
      );
  }, []);

  const install = async () => {
    if (!update) return;
    setInstalling(true);
    setError(undefined);
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch (e) {
      setInstalling(false);
      setError(t("update.failed", { error: String(e) }));
    }
  };

  return (
    <Dialog.Root
      open={!!update}
      onOpenChange={(open) => !open && !installing && setUpdate(undefined)}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed top-1/2 left-1/2 flex max-h-[calc(100vh-3rem)] w-[28rem] -translate-1/2 flex-col gap-4 rounded-lg bg-white p-6 shadow-xl">
          <Dialog.Title className="text-xl font-semibold">{t("update.title")}</Dialog.Title>
          <Dialog.Description className="text-sm">
            {t("update.available", { version: update?.version, current: update?.currentVersion })}
          </Dialog.Description>
          {update?.body && (
            <div className="min-h-0 overflow-y-auto rounded-md bg-zinc-50 p-3 text-xs whitespace-pre-wrap">
              {update.body}
            </div>
          )}
          <p className="text-sm text-zinc-600">{t("update.saveFirst")}</p>
          {error && (
            <p role="alert" className="text-sm text-red-700">
              {error}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Dialog.Close
              disabled={installing}
              className="rounded-md px-4 py-2 text-sm hover:bg-zinc-100 disabled:opacity-50"
            >
              {t("update.later")}
            </Dialog.Close>
            <button
              type="button"
              disabled={installing}
              onClick={() => void install()}
              className="rounded-md bg-zinc-900 px-4 py-2 text-sm text-white disabled:opacity-50"
            >
              {installing ? t("update.installing") : t("update.install")}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
