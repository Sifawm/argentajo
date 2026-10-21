import { describe, expect, it } from "vitest";
import { initialLanguage } from "./language";

describe("initialLanguage", () => {
  it("uses the persisted language when there is one", () => {
    expect(initialLanguage("en", ["es-ES"])).toBe("en");
  });

  it("follows the system language on first launch", () => {
    expect(initialLanguage(null, ["es-AR", "en-US"])).toBe("es");
    expect(initialLanguage(null, ["en-GB"])).toBe("en");
  });

  it("picks the first supported system language", () => {
    expect(initialLanguage(null, ["fr-FR", "es-ES"])).toBe("es");
  });

  it("falls back to English for unsupported or garbage values", () => {
    expect(initialLanguage("klingon", ["fr-FR"])).toBe("en");
    expect(initialLanguage(null, [])).toBe("en");
  });
});
