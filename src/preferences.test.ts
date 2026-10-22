import { createInstance } from "i18next";
import { describe, expect, it } from "vitest";
import es from "./locales/es/common.json";
import { FACTORY, formatSize, fromUnit, newJob, parsePreferences, toUnit } from "./preferences";

describe("parsePreferences", () => {
  it("gives the factory defaults when nothing is saved", () => {
    expect(parsePreferences(null)).toEqual(FACTORY);
    expect(FACTORY).toEqual({
      unit: "mm",
      language: null,
      job: {
        paper: "a4",
        landscape: false,
        printerMargin: 5,
        separation: 1,
        edgeMargin: 1.5,
        timeLimit: 60,
      },
    });
  });

  it("keeps what was saved", () => {
    const saved = {
      unit: "px",
      language: "es",
      job: {
        paper: "letter",
        landscape: true,
        printerMargin: 3,
        separation: 0,
        edgeMargin: 2,
        timeLimit: 10,
      },
    };
    expect(parsePreferences(saved)).toEqual(saved);
  });

  it("falls back field by field on missing or bad values", () => {
    expect(
      parsePreferences({
        unit: "inch",
        language: "klingon",
        job: { paper: "custom", printerMargin: -1, separation: null, timeLimit: 0, edgeMargin: 3 },
      }),
    ).toEqual({ ...FACTORY, job: { ...FACTORY.job, edgeMargin: 3 } });
    expect(parsePreferences("garbage")).toEqual(FACTORY);
  });
});

describe("units", () => {
  it("converts from mm and back", () => {
    expect(toUnit(25, "mm")).toBe(25);
    expect(toUnit(25, "cm")).toBe(2.5);
    expect(toUnit(25.4, "px")).toBe(96);
    expect(fromUnit(2.5, "cm")).toBe(25);
    expect(fromUnit(96, "px")).toBeCloseTo(25.4);
  });

  it("shows typed values back without float noise", () => {
    for (const unit of ["mm", "cm", "px"] as const)
      for (const value of [0.1, 1.3, 100, 793.7])
        expect(toUnit(fromUnit(value, unit), unit)).toBe(value);
  });
});

describe("formatSize", () => {
  it("rounds per unit, in the language's number format", async () => {
    const i18n = createInstance();
    await i18n.init({ lng: "es", resources: { es: { common: es } } });
    expect(formatSize(i18n.t, [210, 297.05], "mm")).toBe("210 × 297,1 mm");
    expect(formatSize(i18n.t, [210, 297.05], "cm")).toBe("21 × 29,71 cm");
    expect(formatSize(i18n.t, [210, 297.05], "px")).toBe("794 × 1123 px");
  });
});

describe("newJob", () => {
  it("starts from the defaults, with one sheet and no pieces", () => {
    expect(newJob({ ...FACTORY.job, paper: "a3", landscape: true, timeLimit: 30 })).toEqual({
      paper: "a3",
      customPaper: [297, 420],
      landscape: true,
      printerMargin: 5,
      sheets: [[100, 50]],
      separation: 1,
      edgeMargin: 1.5,
      timeLimit: 30,
      items: [],
    });
  });
});
