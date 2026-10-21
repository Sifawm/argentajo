import { invoke } from "@tauri-apps/api/core";
import { Languages } from "lucide-react";
import { Tabs, ToggleGroup } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { changeLanguage } from "./i18n";
import JobView from "./JobView";
import { isLanguage, languageNames, languages } from "./language";
import type { PieceView } from "./library";
import LibraryView from "./LibraryView";

const VIEWS = ["library", "job"] as const;

export default function App() {
  const { t, i18n } = useTranslation();
  const [version, setVersion] = useState<string>();
  const [pieces, setPieces] = useState<PieceView[]>([]);

  useEffect(() => {
    invoke<string>("core_version").then(setVersion, () => setVersion("?"));
  }, []);

  return (
    <Tabs.Root defaultValue="library" className="flex h-screen flex-col gap-6 p-6">
      <header className="flex items-center justify-between gap-6">
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">Argentajo</h1>
          <p className="text-sm text-zinc-600">{t("tagline")}</p>
        </div>

        <div className="flex items-center gap-3">
          {version && (
            <span className="font-mono text-xs text-zinc-400">{t("coreVersion", { version })}</span>
          )}
          <Languages aria-hidden className="size-5 text-zinc-500" />
          <ToggleGroup.Root
            type="single"
            aria-label={t("language")}
            value={i18n.resolvedLanguage}
            onValueChange={(value) => isLanguage(value) && changeLanguage(value)}
            className="inline-flex rounded-md border border-zinc-300"
          >
            {languages.map((language) => (
              <ToggleGroup.Item
                key={language}
                value={language}
                className="px-3 py-1 text-sm first:rounded-l-md last:rounded-r-md data-[state=on]:bg-zinc-900 data-[state=on]:text-white"
              >
                {languageNames[language]}
              </ToggleGroup.Item>
            ))}
          </ToggleGroup.Root>
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
  );
}
