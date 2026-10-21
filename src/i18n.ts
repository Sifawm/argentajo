import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { defaultLanguage, initialLanguage, type Language } from "./language";
import enCommon from "./locales/en/common.json";
import esCommon from "./locales/es/common.json";

// One namespace per view; add the view's JSON files here as views appear.
const resources = {
  en: { common: enCommon },
  es: { common: esCommon },
} satisfies Record<Language, unknown>;

const STORAGE_KEY = "language";

function readStoredLanguage(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export function changeLanguage(language: Language) {
  try {
    localStorage.setItem(STORAGE_KEY, language);
  } catch {
    // Not persisted; still switch for this session.
  }
  return i18n.changeLanguage(language);
}

i18n.on("languageChanged", (language) => {
  document.documentElement.lang = language;
});

void i18n.use(initReactI18next).init({
  resources,
  lng: initialLanguage(readStoredLanguage(), navigator.languages),
  fallbackLng: defaultLanguage,
  defaultNS: "common",
  interpolation: { escapeValue: false },
});
