import { invoke } from "@tauri-apps/api/core";
import { Tabs } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import JobView from "./JobView";
import type { PieceView } from "./library";
import LibraryView from "./LibraryView";
import { type Preferences, PreferencesContext } from "./preferences";
import PreferencesDialog from "./PreferencesDialog";
import UpdateDialog from "./UpdateDialog";

const VIEWS = ["library", "job"] as const;

export default function App({ initialPreferences }: { initialPreferences: Preferences }) {
  const { t } = useTranslation();
  const [preferences, setPreferences] = useState(initialPreferences);
  const [version, setVersion] = useState<string>();
  const [pieces, setPieces] = useState<PieceView[]>([]);

  useEffect(() => {
    invoke<string>("core_version").then(setVersion, () => setVersion("?"));
  }, []);

  return (
    <PreferencesContext value={preferences}>
      <Tabs.Root defaultValue="library" className="flex h-screen flex-col gap-6 p-6">
        <header className="flex items-center justify-between gap-6">
          <div>
            <h1 className="text-3xl font-semibold tracking-tight">Argentajo</h1>
            <p className="text-sm text-zinc-600">{t("tagline")}</p>
          </div>

          <div className="flex items-center gap-3">
            {version && (
              <span className="font-mono text-xs text-zinc-400">
                {t("coreVersion", { version })}
              </span>
            )}
            <PreferencesDialog preferences={preferences} setPreferences={setPreferences} />
          </div>
        </header>

        <Tabs.List aria-label={t("views.label")} className="flex gap-1 border-b border-zinc-200">
          {VIEWS.map((view) => (
            <Tabs.Trigger
              key={view}
              value={view}
              className="-mb-px border-b-2 border-transparent px-4 py-2 text-sm data-[state=active]:border-zinc-900 data-[state=active]:font-semibold"
            >
              {t(`views.${view}`)}
            </Tabs.Trigger>
          ))}
        </Tabs.List>

        {/* Both stay mounted, so the Job survives a visit to the Library. */}
        <Tabs.Content
          value="library"
          forceMount
          className="flex min-h-0 flex-1 flex-col overflow-y-auto data-[state=inactive]:hidden"
        >
          <LibraryView pieces={pieces} setPieces={setPieces} />
        </Tabs.Content>
        <Tabs.Content
          value="job"
          forceMount
          className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden"
        >
          <JobView pieces={pieces} />
        </Tabs.Content>
      </Tabs.Root>
      <UpdateDialog />
    </PreferencesContext>
  );
}
