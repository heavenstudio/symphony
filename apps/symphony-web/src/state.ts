// The pure half of following the feed: how long to wait before reading again, when another client
// build loads the page again, and when the frame's metadata is read again.

/** The first wait before a lost feed is tried again, in ms. */
const FIRST_WAIT_MS = 1_000;
/** The longest wait between tries, in ms; a stream that held this long starts the waits again. */
const LAST_WAIT_MS = 30_000;

/** How a page's feed stream ended: lost after holding `held` ms (0 if it never opened), or away. */
export type Ended = { lost: number } | "away";

/**
 * The waits between tries of a lost feed: 1 s, doubling to 30 s, until a stream holds 30 s. A
 * stream that ends as soon as it opens does not start them again, so a server that keeps closing
 * it is asked ever less often.
 */
export class Backoff {
  private next = FIRST_WAIT_MS;

  /** The wait, in ms, after a stream that held `held` ms (0 when it never opened). */
  after(held: number): number {
    if (held >= LAST_WAIT_MS) this.next = FIRST_WAIT_MS;
    const wait = this.next;
    this.next = Math.min(wait * 2, LAST_WAIT_MS);
    return wait;
  }

  /**
   * The wait, in ms, before a page reads again after its stream `ended`. A stream the page closed
   * because it went away failed nothing: none, and the waits start again.
   */
  wait(ended: Ended): number {
    if (ended === "away") {
      this.next = FIRST_WAIT_MS;
      return 0;
    }
    return this.after(ended.lost);
  }
}

/**
 * Whether a client whose own build is `own` loads the page again for the board naming `served`:
 * only for another build, and once per build (`tried` is the one it last loaded again for), so a
 * shell the client cannot match never loads it in a loop. A client that cannot name its own
 * build never does.
 */
export function reloadFor(own: string | null, served: string, tried: string | null): boolean {
  return own !== null && own !== served && tried !== served;
}

/**
 * Whether a read of `/api/activity` leaves the persistent panel's baseline where it is instead of
 * moving it to `known`, the ids the page knew before this read: until the panel is ready, `known`
 * holding an id the baseline lacks means an earlier read already announced that arrival.
 */
export function keepsPanelBaseline(
  baseline: unknown[] | null,
  known: unknown[] | null,
  panelReady: boolean,
): boolean {
  if (panelReady || baseline === null || known === null) return false;
  return known.some((id) => !baseline.includes(id));
}

/**
 * The frame's metadata reads: the first read coalesces with the first feed handshake; a reconnect
 * still asks for a fresh read, and a failed first read is tried once more after that handshake.
 */
export class MetadataReads {
  private connections = 0;
  private active = false;
  private ready = false;

  /** The feed opened: whether the metadata is read again. */
  connected(): boolean {
    this.connections += 1;
    return this.connections > 1 || (!this.active && !this.ready);
  }

  /** A read starts: the connections it is read after. */
  start(): number {
    this.active = true;
    return this.connections;
  }

  /** The current read ended: whether it is tried again. */
  finished(started: number, success: boolean): boolean {
    this.active = false;
    this.ready = success;
    return !success && started < this.connections;
  }
}
