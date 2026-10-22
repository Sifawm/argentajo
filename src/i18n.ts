import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { defaultLanguage, initialLanguage, type Language } from "./language";
import enCommon from "./locales/en/common.json";
import enJob from "./locales/en/job.json";
import enLibrary from "./locales/en/library.json";
import esCommon from "./locales/es/common.json";
import esJob from "./locales/es/job.json";
import esLibrary from "./locales/es/library.json";

// One namespace per view; add the view's JSON files here as views appear.
const resources = {
  en: { common: enCommon, library: enLibrary, job: enJob },
  es: { common: esCommon, library: esLibrary, job: esJob },
} satisfies Record<Language, unknown>;

i18n.on("languageChanged", (language) => {
  document.documentElement.lang = language;
});

void i18n.use(initReactI18next).init({
  resources,
  lng: initialLanguage(null, navigator.languages),
  fallbackLng: defaultLanguage,
  defaultNS: "common",
  interpolation: { escapeValue: false },
});

/** Switches to the preferred language, or the system's when there is none. */
export const applyLanguage = (preferred: Language | null) =>
  i18n.changeLanguage(initialLanguage(preferred, navigator.languages));
