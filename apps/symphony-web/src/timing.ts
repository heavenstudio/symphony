// The client's waits by a length of time, each named for the one thing RULES.md
// `code/no-timed-waits` lets a number pace: a retry's wait between two tries (its length
// `Backoff`'s, state.ts), a deadline that only bounds a wait for an event and ends it with words,
// a debounce's rest, and a clock's tick (a time the page shows). Everything else waits for its
// event (a message, a promise, an observer, a listener) and acts at once: a page's update never
// waits a length of time. This is the one module that may call `setTimeout` and `setInterval`
// (biome.json), and `sleep`, the plain wait, is imported only where a site is kept in the rule's
// baseline until it waits for its event, each such file saying so at its import.

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
 * The wait between two tries of a read, a stream or a save, its length from `Backoff` (1 s
 * doubling to 30 s): `ms` later, or at once when `signal` ends.
 */
export function retryWait(ms: number, signal?: AbortSignal): Promise<void> {
  return wait(ms, signal);
}

/**
 * A debounce's rest: `ms` later, or at once when `signal` ends. What waited then acts only when no
 * newer call took its place meanwhile, so a burst of calls acts once, after its last.
 */
export function debounce(ms: number, signal?: AbortSignal): Promise<void> {
  return wait(ms, signal);
}

/** A clock's tick: `ms` later, when a time the page shows next changes, or at once when `signal`
 * ends. */
export function tick(ms: number, signal?: AbortSignal): Promise<void> {
  return wait(ms, signal);
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
