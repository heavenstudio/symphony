import { describe, expect, it } from "vitest";
import { deviceNotice, localMoment, moment, parseMoment, plural, savedAt } from "./words";

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

describe("a noun", () => {
  it("takes an s unless there is one", () => {
    expect(plural(1, "card")).toBe("card");
    expect(plural(0, "card")).toBe("cards");
    expect(plural(2, "document")).toBe("documents");
  });
});

describe("a register's moment", () => {
  const minutes = (text: string) => {
    const read = parseMoment(text);
    if (read === null) throw new Error(`${text} does not parse`);
    return read;
  };

  it("reads a date with or without its time, across a year and a leap day", () => {
    expect(minutes("2026-10-08 07:42") - minutes("2026-10-08")).toBe(7 * 60 + 42);
    expect(minutes("2026-10-08") - minutes("2026-10-07")).toBe(24 * 60);
    expect(minutes("2025-01-01") - minutes("2024-12-31")).toBe(24 * 60);
    expect(minutes("2024-03-01") - minutes("2024-02-28")).toBe(2 * 24 * 60);
  });

  it("counts minutes from 1970 on the proleptic calendar, before it too", () => {
    expect(minutes("1970-01-01")).toBe(0);
    expect(minutes("1970-01-02 00:01")).toBe(1441);
    expect(minutes("1969-12-31 23:59")).toBe(-1);
    expect(minutes("2000-03-01") - minutes("2000-02-28")).toBe(2 * 24 * 60);
    expect(minutes("1900-03-01") - minutes("1900-02-28")).toBe(24 * 60);
    expect(minutes("0001-01-01")).toBe(-719_162 * 1440);
  });

  it.each([
    "",
    "yesterday",
    "2026-13-01",
    "2026-00-10",
    "2026-10-32",
    "2026-10-00",
    "2026-10-08 25:00",
    "2026-10-08 07:60",
    "2026-10-08T07:42",
    "2026-10-08 07:42:00",
    "2026-10-08 0742",
    "2026-10-08 7:42",
    "2026-10-08 ",
    "26-10-08",
    "2026-1-08",
    "2026-10-8",
    "2026-10-08-01",
    "2026/10/08",
    "2026-1a-08",
    "2026-10-08 -1:00",
    "2026--1-08",
  ])("is none for %j", (text) => {
    expect(parseMoment(text)).toBeNull();
  });

  it("reads each number as Rust does, a plus before it allowed", () => {
    expect(minutes("2026-+1-08")).toBe(minutes("2026-01-08"));
    expect(minutes("2026-10-08 +7:+2")).toBe(minutes("2026-10-08 07:02"));
    expect(minutes("+026-01-01")).toBe(minutes("0026-01-01"));
    expect(parseMoment("-001-03-01")).toBeNull();
  });

  it("is none for a part out of range", () => {
    expect(moment(2026, 12, 31, 23, 59)).toBe(minutes("2026-12-31 23:59"));
    expect(moment(2026, 13, 1, 0, 0)).toBeNull();
    expect(moment(2026, 1, 32, 0, 0)).toBeNull();
    expect(moment(2026, 1, 1, 24, 0)).toBeNull();
    expect(moment(2026, 1, 1, 0, 60)).toBeNull();
  });

  it("is the wall clock's where the owner is", () => {
    expect(localMoment(at(2026, 10, 8, 7, 42))).toBe(minutes("2026-10-08 07:42"));
    expect(localMoment(at(1999, 12, 31, 23, 59))).toBe(minutes("1999-12-31 23:59"));
  });
});
