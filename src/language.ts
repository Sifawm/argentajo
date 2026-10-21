export const languages = ["es", "en"] as const;
export type Language = (typeof languages)[number];
export const defaultLanguage: Language = "en";

/** Each language's name in itself, so it is never translated. */
export const languageNames: Record<Language, string> = { es: "Español", en: "English" };

export const isLanguage = (value: string | null): value is Language =>
  languages.includes(value as Language);

/** The persisted choice wins; otherwise the first supported system language; otherwise the default. */
export function initialLanguage(stored: string | null, system: readonly string[]): Language {
  if (isLanguage(stored)) return stored;
  return system.map((tag) => tag.split("-")[0]).find(isLanguage) ?? defaultLanguage;
}
