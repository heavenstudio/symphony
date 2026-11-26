import { describe, expect, it } from "vitest";
import { BAND_SHARE, Cuts, MOST_SHARE } from "./timeline-cut";

/** Whole hours of 10 h from 0, an hour from 4 h to 5 h... in seconds. */
const HOUR = 3600;
const WHOLE = { from: 0, to: 10 * HOUR };
/** A band a tenth of the track, so its arithmetic reads in round numbers. */
const SHARE = 0.1;
/** Skipped: 2 h to 6 h. */
const ONE = new Cuts(WHOLE, { skipped: [{ from: 2 * HOUR, to: 6 * HOUR }], share: SHARE });

describe("the cut scale", () => {
  it("with nothing skipped is the plain clock", () => {
    const plain = new Cuts(WHOLE);
    expect([plain.skipped, plain.share]).toEqual([[], 0]);
    expect(plain.span(WHOLE)).toBe(10 * HOUR);
    expect(plain.most).toBe(10 * HOUR);
    expect(plain.at(WHOLE, 2.5 * HOUR)).toBe(0.25);
    expect(plain.time(WHOLE, 0.25)).toBe(2.5 * HOUR);
    const view = { from: HOUR, to: 3 * HOUR };
    expect(plain.at(view, 2 * HOUR)).toBe(0.5);
    expect(plain.time(view, 0.5)).toBe(2 * HOUR);
    expect(plain.window(2 * HOUR, HOUR)).toEqual(view);
    // A share with nothing to skip, or nothing skipped at no share, is no cut.
    expect(new Cuts(WHOLE, { skipped: [], share: SHARE }).share).toBe(0);
    expect(new Cuts(WHOLE, { skipped: [{ from: 0, to: HOUR }], share: 0 }).skipped).toEqual([]);
  });

  it("gives a band its share of the track and the work the rest, as long as the hours either side", () => {
    // Six hours of work and one band: each hour of work 0.9 / 6 of the track.
    expect(ONE.span(WHOLE)).toBeCloseTo((6 * HOUR) / 0.9, 6);
    expect(ONE.at(WHOLE, 2 * HOUR)).toBeCloseTo(0.3, 9);
    // The band's middle, then its end, a tenth of the track on.
    expect(ONE.at(WHOLE, 4 * HOUR)).toBeCloseTo(0.35, 9);
    expect(ONE.at(WHOLE, 6 * HOUR)).toBeCloseTo(0.4, 9);
    expect(ONE.at(WHOLE, 10 * HOUR)).toBeCloseTo(1, 9);
    // And back.
    for (const x of [0, 0.15, 0.3, 0.35, 0.4, 0.7, 1]) {
      expect(ONE.at(WHOLE, ONE.time(WHOLE, x))).toBeCloseTo(x, 9);
    }
    expect(ONE.time(WHOLE, 0.35)).toBeCloseTo(4 * HOUR, 6);
  });

  it("gives a band partly in a view as much of its share as of its hours", () => {
    // From 1 h to 4 h: an hour of work and half the band, a twentieth of the track.
    const view = { from: HOUR, to: 4 * HOUR };
    expect(ONE.bandsIn(view.from, view.to)).toBe(0.5);
    expect(ONE.workIn(view.from, view.to)).toBe(HOUR);
    expect(ONE.at(view, 2 * HOUR)).toBeCloseTo(0.95, 9);
    expect(ONE.at(view, 4 * HOUR)).toBeCloseTo(1, 9);
    // A zoom holds less work on a track, and the band as wide.
    const zoomed = { from: 1.5 * HOUR, to: 6.5 * HOUR };
    expect(ONE.at(zoomed, 6 * HOUR) - ONE.at(zoomed, 2 * HOUR)).toBeCloseTo(SHARE, 9);
  });

  it("counts what is skipped, the bands and the work between two instants, either way", () => {
    expect(ONE.skippedIn(0, 10 * HOUR)).toBe(4 * HOUR);
    expect(ONE.skippedIn(10 * HOUR, 0)).toBe(-4 * HOUR);
    expect(ONE.bandsIn(3 * HOUR, 0)).toBe(-0.25);
    expect(ONE.workIn(0, 3 * HOUR)).toBe(2 * HOUR);
    expect(ONE.skippedIn(7 * HOUR, 8 * HOUR)).toBe(0);
  });

  it("goes on as the clock does past either end of the whole hours", () => {
    const track = ONE.most;
    expect(ONE.instant(track, -HOUR)).toBe(-HOUR);
    expect(ONE.instant(track, ONE.length(track) + HOUR)).toBeCloseTo(11 * HOUR, 6);
    expect(ONE.axis(track, -HOUR)).toBe(-HOUR);
  });

  it("keeps the stretches inside the whole hours, apart, and gives many bands less each", () => {
    const messy = new Cuts(WHOLE, {
      skipped: [
        { from: 5 * HOUR, to: 7 * HOUR },
        { from: HOUR, to: 3 * HOUR },
        { from: 2 * HOUR, to: 4 * HOUR },
        { from: 9 * HOUR, to: 11 * HOUR },
        { from: 8 * HOUR, to: 8 * HOUR },
      ],
      share: SHARE,
    });
    expect(messy.skipped).toEqual([
      { from: HOUR, to: 3 * HOUR },
      { from: 5 * HOUR, to: 7 * HOUR },
    ]);
    const many = new Cuts(WHOLE, {
      skipped: Array.from({ length: 20 }, (_, n) => ({
        from: (n / 2) * HOUR,
        to: (n / 2 + 0.25) * HOUR,
      })),
      share: BAND_SHARE,
    });
    expect(many.skipped).toHaveLength(20);
    expect(many.share).toBeCloseTo(MOST_SHARE / 20, 12);
    expect(new Cuts(WHOLE, { skipped: [{ from: 0, to: HOUR }], share: BAND_SHARE }).share).toBe(
      BAND_SHARE,
    );
  });

  it("holds no work where a view is all band", () => {
    expect(ONE.span({ from: 3 * HOUR, to: 5 * HOUR })).toBe(0);
  });
});
