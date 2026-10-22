import { describe, expect, it } from "vitest";
import { fromJobRequest, type JobRequest, toJobRequest } from "./job";

const request = (sizeMm: [number, number]): JobRequest => ({
  paper: { sizeMm, marginMm: 4 },
  sheetsMm: [
    [80, 50],
    [60, 40],
  ],
  separationMm: 1.2,
  edgeMarginMm: 2,
  timeLimitS: 30,
  items: [
    { id: "job-0", quantity: { fixed: 3 } },
    { id: "job-1", quantity: "fill" },
  ],
});

describe("fromJobRequest", () => {
  it("gives back settings that make the very same request", () => {
    // Compared as JSON, as a Layout's request is to tell whether it's stale.
    for (const size of [
      [210, 297],
      [297, 210],
      [100, 150],
      [150, 100],
    ] as [number, number][]) {
      expect(JSON.stringify(toJobRequest(fromJobRequest(request(size))))).toBe(
        JSON.stringify(request(size)),
      );
    }
  });

  it("recognises the paper presets and their orientation", () => {
    expect(fromJobRequest(request([297, 210]))).toMatchObject({ paper: "a4", landscape: true });
    expect(fromJobRequest(request([215.9, 279.4]))).toMatchObject({
      paper: "letter",
      landscape: false,
    });
    expect(fromJobRequest(request([150, 100]))).toMatchObject({
      paper: "custom",
      customPaper: [100, 150],
      landscape: true,
    });
  });

  it("gives fill items a quantity of 1 for when they're switched back", () => {
    expect(fromJobRequest(request([210, 297])).items).toEqual([
      { id: "job-0", quantity: 3, fill: false },
      { id: "job-1", quantity: 1, fill: true },
    ]);
  });
});
