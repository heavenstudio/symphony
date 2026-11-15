// What a page that reads the board's lists while its feed runs keeps of the feed meanwhile: the task
// board's and the agents page's, so no other kind carries it. An answer read while events came is
// drawn with them replayed onto it, and a live update keeps what the reader looks at in place.

/**
 * The feed's events since a read began, for each read still out: an answer read meanwhile is drawn
 * with them replayed onto it. Only the events some read still needs are kept.
 */
export interface Journal {
  version: number;
  events: [number, unknown][];
  /** How many reads are out since each version. */
  readers: Map<number, number>;
}

/** A read's place in the journal, from its start until it is released, answered or not. */
export class ReadCursor {
  readonly since: number;
  private out = true;

  constructor(private readonly journal: Journal) {
    this.since = journal.version;
    journal.readers.set(this.since, (journal.readers.get(this.since) ?? 0) + 1);
  }

  /** The events heard since the read began, in order. */
  events(): unknown[] {
    return this.journal.events
      .filter(([version]) => version > this.since)
      .map(([, value]) => value);
  }

  /** The read is over: the events no read still out needs are dropped. */
  release(): void {
    if (!this.out) return;
    this.out = false;
    const { journal } = this;
    const readers = (journal.readers.get(this.since) ?? 1) - 1;
    if (readers > 0) journal.readers.set(this.since, readers);
    else journal.readers.delete(this.since);
    if (journal.readers.size === 0) {
      journal.events = [];
      return;
    }
    const oldest = Math.min(...journal.readers.keys());
    journal.events = journal.events.filter(([version]) => version > oldest);
  }
}

/** A live update about to be drawn keeps what the reader looks at in place (shell.js). */
export function holdPlace(rows: string): void {
  (window as { reviewHoldPlace?: (rows: string) => void }).reviewHoldPlace?.(rows);
}
