import { Settings } from "lucide-react";
import { Dialog, ToggleGroup } from "radix-ui";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import { applyLanguage } from "./i18n";
import { PAPERS } from "./job";
import { isLanguage, languageNames, languages } from "./language";
import {
  fromUnit,
  isPositive,
  isSpacing,
  type JobDefaults,
  type Preferences,
  savePreferences,
  toUnit,
  UNITS,
} from "./preferences";

const TOGGLE_ITEM =
  "px-3 py-1 text-sm first:rounded-l-md last:rounded-r-md data-[state=on]:bg-zinc-900 data-[state=on]:text-white";

interface Props {
  preferences: Preferences;
  setPreferences: (preferences: Preferences) => void;
}

/** Behind the gear button. Every change applies and is saved at once. */
export default function PreferencesDialog({ preferences, setPreferences }: Props) {
  const { t, i18n } = useTranslation();
  const [error, setError] = useState<string>();
  const { unit, job } = preferences;

  const change = (next: Preferences) => {
    setPreferences(next);
    setError(undefined);
    savePreferences(next).catch((e) => setError(t("preferences.saveFailed", { error: String(e) })));
  };
  const changeJob = (changes: Partial<JobDefaults>) =>
    change({ ...preferences, job: { ...job, ...changes } });

  // Uncontrolled, so a half-typed or invalid value can sit in the field without being kept.
  const numberInput = (
    label: string,
    value: number,
    valid: (value: number) => boolean,
    onChange: (value: number) => void,
  ) => (
    <label className="flex flex-col gap-1 text-sm">
      {label}
      <input
        key={unit}
        type="number"
        min="0"
        step="any"
        defaultValue={value}
        onChange={(e) => valid(e.target.valueAsNumber) && onChange(e.target.valueAsNumber)}
        className="w-28 rounded-md border border-zinc-300 px-2 py-1 invalid:border-red-700"
      />
    </label>
  );
  const lengthInput = (label: string, key: "printerMargin" | "separation" | "edgeMargin") =>
    numberInput(label, toUnit(job[key], unit), isSpacing, (value) =>
      changeJob({ [key]: fromUnit(value, unit) }),
    );

  return (
    <Dialog.Root>
      <Dialog.Trigger
        aria-label={t("preferences.title")}
        title={t("preferences.title")}
        className="rounded-md p-1.5 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-900"
      >
        <Settings aria-hidden className="size-5" />
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed top-1/2 left-1/2 flex max-h-[calc(100vh-3rem)] w-[28rem] -translate-1/2 flex-col gap-5 overflow-y-auto rounded-lg bg-white p-6 shadow-xl"
        >
          <Dialog.Title className="text-xl font-semibold">{t("preferences.title")}</Dialog.Title>

          <Field label={t("preferences.unit")} hint={t("preferences.unitHint")}>
            <ToggleGroup.Root
              type="single"
              aria-label={t("preferences.unit")}
              value={unit}
              onValueChange={(value) =>
                value && change({ ...preferences, unit: value as Preferences["unit"] })
              }
              className="inline-flex self-start rounded-md border border-zinc-300"
            >
              {UNITS.map((value) => (
                <ToggleGroup.Item key={value} value={value} className={TOGGLE_ITEM}>
                  {value}
                </ToggleGroup.Item>
              ))}
            </ToggleGroup.Root>
          </Field>

          <Field label={t("preferences.language")}>
            <ToggleGroup.Root
              type="single"
              aria-label={t("preferences.language")}
              value={preferences.language ?? i18n.resolvedLanguage}
              onValueChange={(value) => {
                if (!isLanguage(value)) return;
                change({ ...preferences, language: value });
                void applyLanguage(value);
              }}
              className="inline-flex self-start rounded-md border border-zinc-300"
            >
              {languages.map((language) => (
                <ToggleGroup.Item key={language} value={language} className={TOGGLE_ITEM}>
                  {languageNames[language]}
                </ToggleGroup.Item>
              ))}
            </ToggleGroup.Root>
          </Field>

          <fieldset className="flex flex-col gap-3">
            <legend className="mb-1 font-semibold">{t("preferences.job")}</legend>
            <p className="text-xs text-zinc-500">{t("preferences.jobHint")}</p>
            <div className="flex items-end gap-3">
              <label className="flex flex-col gap-1 text-sm">
                {t("job:paperSize")}
                <select
                  value={job.paper}
                  onChange={(e) => changeJob({ paper: e.target.value as JobDefaults["paper"] })}
                  className="rounded-md border border-zinc-300 px-2 py-1"
                >
                  {Object.keys(PAPERS).map((name) => (
                    <option key={name} value={name}>
                      {t(`job:papers.${name}`)}
                    </option>
                  ))}
                </select>
              </label>
              <ToggleGroup.Root
                type="single"
                aria-label={t("job:orientation")}
                value={job.landscape ? "landscape" : "portrait"}
                onValueChange={(value) => value && changeJob({ landscape: value === "landscape" })}
                className="inline-flex rounded-md border border-zinc-300"
              >
                {(["portrait", "landscape"] as const).map((value) => (
                  <ToggleGroup.Item key={value} value={value} className={TOGGLE_ITEM}>
                    {t(`job:${value}`)}
                  </ToggleGroup.Item>
                ))}
              </ToggleGroup.Root>
            </div>
            <div className="flex flex-wrap gap-3">
              {lengthInput(t("job:printerMargin", { unit }), "printerMargin")}
              {lengthInput(t("job:separation", { unit }), "separation")}
              {lengthInput(t("job:edgeMargin", { unit }), "edgeMargin")}
              {numberInput(t("job:timeLimit"), job.timeLimit, isPositive, (timeLimit) =>
                changeJob({ timeLimit }),
              )}
            </div>
          </fieldset>

          {error && (
            <p role="alert" className="text-sm text-red-700">
              {error}
            </p>
          )}
          <Dialog.Close className="self-end rounded-md bg-zinc-900 px-4 py-2 text-sm text-white">
            {t("preferences.close")}
          </Dialog.Close>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2 text-sm">
      <span className="font-semibold">{label}</span>
      {children}
      {hint && <span className="text-xs text-zinc-500">{hint}</span>}
    </div>
  );
}
