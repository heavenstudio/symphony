// The client's waits by a length of time, each named for the one thing RULES.md
// `code/no-timed-waits` lets a number pace, and each shaped to it, so none is a plain wait a fixed
// poll could be written with: a retry's wait between two tries (its length the next of a
// `Backoff`'s, state.ts), a deadline that only bounds a wait for an event and ends it with words, a
// debounce, whose calls take each other's place, and a clock's tick, to the next minute a page
// shows. Everything else waits for its event (a message, a promise, an observer, a listener) and
// acts at once: a page's update never waits a length of time. This is the one module that may call
// `setTimeout` and `setInterval` (biome.json), and `sleep`, the plain wait, is imported only where
// a site is kept in the rule's baseline until it waits for its event, each such file saying so at
// its import.

import type { Backoff, Ended } from "./state";

/** `ms` later, or at once when `signal` ends, or had. */
function wait(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal?.aborted) return resolve();
    const done = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    signal?.addEventListener("abort", done);
  });
}

/**
 * The wait between two tries of a read, a stream or a save: the next of `backoff`'s waits after a
 * try that `ended` so (1 s doubling to 30 s, state.ts), or at once when `signal` ends.
 */
export function retryWait(backoff: Backoff, ended: Ended, signal?: AbortSignal): Promise<void> {
  return wait(backoff.wait(ended), signal);
}

/**
 * A debounce: of a burst of rests, only the last acts, once no other has come for `ms`. Each rest
 * takes the place of the one before it, which is over at once and acts not; `cancel`, and `signal`
 * ending, end the one under way the same way.
 */
export class Debounce {
  /** The rest under way, until it acts or another takes its place. */
  private resting: AbortController | null = null;

  constructor(
    private readonly ms: number,
    private readonly signal?: AbortSignal,
  ) {}

  /** A rest: whether it is the one that acts, once no later rest came for the debounce's length;
   * false at once when one does, or the debounce is cancelled or its signal ends. */
  rest(): Promise<boolean> {
    this.cancel();
    if (this.signal?.aborted) return Promise.resolve(false);
    const resting = new AbortController();
    this.resting = resting;
    this.signal?.addEventListener("abort", () => resting.abort(), { signal: resting.signal });
    return wait(this.ms, resting.signal).then(() => {
      if (resting.signal.aborted) return false;
      // Over, and its listener on `signal` with it.
      resting.abort();
      this.resting = null;
      return true;
    });
  }

  /** The rest under way, if any, over and acting not. */
  cancel(): void {
    this.resting?.abort();
    this.resting = null;
  }
}

/** The length of a minute, by which the times a page shows change. */
const MINUTE_MS = 60_000;

/** A clock's tick: at the next minute, when a time the page shows next changes, or at once when
 * `signal` ends. */
export function tick(signal?: AbortSignal): Promise<void> {
  return wait(MINUTE_MS - (Date.now() % MINUTE_MS), signal);
}

/**
 * A deadline on a wait for an event: `passed` is called once `ms` have passed with the wait not
 * over, which it then ends and says why. The function it gives ends the deadline, as the event
 * came first.
 */
export function deadline(ms: number, passed: () => void): () => void {
  const timer = setTimeout(passed, ms);
  return () => clearTimeout(timer);
}

/**
 * The plain wait, which RULES.md `code/no-timed-waits` forbids: kept for the baseline's sites alone
 * until each waits for its event (biome.json refuses its import elsewhere).
 */
export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return wait(ms, signal);
}
