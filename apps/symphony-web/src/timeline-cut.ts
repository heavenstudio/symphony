// The cut scale, with no DOM: a task's whole hours with the quiet stretches its wide timeline skips
// (timeline-quiet.ts), each a band of one share of the track however far the view is zoomed, the
// work between them scaled to the rest. Every place in the lanes (timeline-layout.ts's `Scale`),
// every gesture (timeline-zoom.ts) and the readout go through it; with nothing skipped it is the
// plain clock, every fraction of the track as many seconds.
//
// The hours lie on an axis measured in seconds of work, at a zoom: where a track holds `track`
// seconds of work, a band counts as its share of them, a band partly in a view as much of its share
// as of its hours there. A view is a window one track long on that axis. The whole task holds the
// most on a track; zooming in holds less and leaves a band as wide, so the work gets the width.
//
// A band is BAND_SHARE of the track: 36 px of the 1,095 px track the task page has at 1440 wide
// and wider, as many of a narrower one. The bands of a task take at most MOST_SHARE of it together,
// each narrower where it has many.

import type { View } from "./timeline-layout";

/** A band's share of the track. */
export const BAND_SHARE = 0.033;
/** The most of the track the bands of a task take together. */
export const MOST_SHARE = 0.3;

/** A stretch the lanes skip, as a band. */
export interface Skip {
  from: number;
  to: number;
}

/** The stretches a view skips, and each band's share of the track. */
export interface Cut {
  skipped: readonly Skip[];
  share: number;
}

/** `whole` cut where `cut` skips, each band `share` of the track. */
export class Cuts {
  /** The stretches skipped inside the whole hours, oldest first, none overlapping. */
  readonly skipped: readonly Skip[];
  /** Each band's share of the track, MOST_SHARE among them all at most. */
  readonly share: number;

  constructor(
    readonly whole: View,
    cut?: Cut | null,
  ) {
    const inside = [...(cut?.skipped ?? [])]
      .filter((skip) => skip.from >= whole.from && skip.to <= whole.to && skip.to > skip.from)
      .sort((a, b) => a.from - b.from);
    const apart: Skip[] = [];
    for (const skip of inside) {
      const last = apart[apart.length - 1];
      if (last === undefined || skip.from >= last.to) apart.push(skip);
    }
    const share = cut?.share ?? 0;
    this.skipped = share > 0 ? apart : [];
    this.share = this.skipped.length === 0 ? 0 : Math.min(share, MOST_SHARE / this.skipped.length);
  }

  /** The seconds skipped from `a` to `b`, less than 0 where `b` is before `a`. */
  skippedIn(a: number, b: number): number {
    if (b < a) return -this.skippedIn(b, a);
    return this.skipped.reduce(
      (sum, skip) => sum + Math.max(0, Math.min(b, skip.to) - Math.max(a, skip.from)),
      0,
    );
  }

  /** How many bands are from `a` to `b`, each counted by the share of its hours there. */
  bandsIn(a: number, b: number): number {
    if (b < a) return -this.bandsIn(b, a);
    return this.skipped.reduce(
      (sum, skip) =>
        sum + Math.max(0, Math.min(b, skip.to) - Math.max(a, skip.from)) / (skip.to - skip.from),
      0,
    );
  }

  /** The seconds of work from `a` to `b`: the clock's, the stretches skipped left out. */
  workIn(a: number, b: number): number {
    return b - a - this.skippedIn(a, b);
  }

  /** The seconds of work one track holds where `view` fills it: its work, the bands in it taking
   * their share of the track; none where it holds no work. */
  span(view: View): number {
    const work = this.workIn(view.from, view.to);
    return work > 0 ? work / (1 - this.share * this.bandsIn(view.from, view.to)) : 0;
  }

  /** The most a track holds: the whole hours on one track. */
  get most(): number {
    return this.span(this.whole);
  }

  /** Where `at` is on the axis where a track holds `track` seconds of work, in seconds of work
   * from the whole hours' start, each band counted as its share of the track. */
  axis(track: number, at: number): number {
    return (
      this.workIn(this.whole.from, at) + this.share * track * this.bandsIn(this.whole.from, at)
    );
  }

  /** The instant `u` along the axis where a track holds `track` seconds, the inverse of `axis`:
   * past either end of the whole hours, as the clock goes on. */
  instant(track: number, u: number): number {
    const band = this.share * track;
    let at = this.whole.from;
    let gone = 0;
    if (u <= 0) return at + u;
    for (const skip of this.skipped) {
      const work = skip.from - at;
      if (u <= gone + work) return at + (u - gone);
      gone += work;
      if (u <= gone + band) return skip.from + ((u - gone) / band) * (skip.to - skip.from);
      gone += band;
      at = skip.to;
    }
    return at + (u - gone);
  }

  /** How long the axis is where a track holds `track` seconds. */
  length(track: number): number {
    return this.axis(track, this.whole.to);
  }

  /** The view one track long from `u` on the axis where a track holds `track` seconds. */
  window(track: number, u: number): View {
    return { from: this.instant(track, u), to: this.instant(track, u + track) };
  }

  /** Where `at` is in `view`, a fraction of the track from its start, outside 0 to 1 beyond it. */
  at(view: View, at: number): number {
    const track = this.span(view);
    return (this.axis(track, at) - this.axis(track, view.from)) / track;
  }

  /** The instant `x` of the track into `view`. */
  time(view: View, x: number): number {
    const track = this.span(view);
    return this.instant(track, this.axis(track, view.from) + x * track);
  }
}
