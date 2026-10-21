import { invoke } from "@tauri-apps/api/core";
import { Languages } from "lucide-react";
import { ToggleGroup } from "radix-ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { changeLanguage } from "./i18n";
import { isLanguage, languageNames, languages } from "./language";

export default function App() {
  const { t, i18n } = useTranslation();
  const [version, setVersion] = useState<string>();

  useEffect(() => {
    invoke<string>("core_version").then(setVersion, () => setVersion("?"));
  }, []);

  return (
    <main className="mx-auto flex min-h-screen max-w-xl flex-col justify-center gap-6 p-8">
      <header>
        <h1 className="text-4xl font-semibold tracking-tight">Argentajo</h1>
        <p className="mt-2 text-zinc-600">{t("tagline")}</p>
      </header>

      {version && <p className="font-mono text-sm">{t("coreVersion", { version })}</p>}

      <div className="flex items-center gap-3">
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
    </main>
  );
}
