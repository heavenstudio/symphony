import { describe, expect, it } from "vitest";
import { deviceNotice, savedAt } from "./words";

/** A moment on the wall clock where the tests run. */
const at = (year: number, month: number, day: number, hours: number, minutes: number) =>
  new Date(year, month - 1, day, hours, minutes);

describe("when a copy was saved", () => {
  const now = at(2026, 11, 14, 9, 5);

  it("is its time the same day", () => {
    expect(savedAt(at(2026, 11, 14, 0, 7), now)).toBe("00:07");
  });

  it("is yesterday's the day before, across a month too", () => {
    expect(savedAt(at(2026, 11, 13, 23, 59), now)).toBe("yesterday 23:59");
    expect(savedAt(at(2026, 10, 31, 8, 0), at(2026, 11, 1, 1, 0))).toBe("yesterday 08:00");
  });

  it("is its day and month earlier in the year, with its year before it", () => {
    expect(savedAt(at(2026, 11, 3, 14, 32), now)).toBe("3 Nov 14:32");
    expect(savedAt(at(2025, 12, 31, 14, 32), at(2026, 1, 1, 9, 0))).toBe("yesterday 14:32");
    expect(savedAt(at(2025, 1, 2, 14, 32), now)).toBe("2 Jan 2025 14:32");
  });
});

describe("what a page drawn from this device says", () => {
  it("is that it checks, or that the server is not reachable", () => {
    expect(deviceNotice("14:32", false)).toBe(
      "Shown from this device, saved 14:32. Checking for changes…",
    );
    expect(deviceNotice("14:32", true)).toBe("Not reachable: shown from this device, saved 14:32");
  });
});
