// A page's parts, each drawn from a read of its own (docs/design/analysis/components-draw-when-their-
// data-arrives.md): a component names the part it draws (`usePart`), and the page's one store
// draws the copy this device kept of it at once, marked stale, reads it naming that copy's tag,
// one read a key however many components name it, tries a failed read again on the feed's waits
// (state.ts's `tryAgain`), and takes the feed: an event after the part's answer is applied to it
// or reads it again, as the part says (`hear`), and one at or before the answer is dropped.
//
// Every read's answer names the feed's cursor it was read at (request.ts's `getAt`): every event
// at or before it is in the answer, one after it may be. So the reads and the feed start in any
// order. An answer is drawn with the events heard since it was asked that came after its cursor
// applied to it again; it is read once more where the store cannot tell what it missed: its
// cursor is before the one the feed opened from, the server named none and the feed opened while
// it was out, or the events it would need were dropped from the few the store keeps. What the
// page shows from this device (cache.ts's `Freshness`) follows from the parts: one drawn from the
// device waits until the server answers it.

import { createContext, useCallback, useContext, useSyncExternalStore } from "react";
import {
  answered,
  drawn,
  FRESH,
  type Freshness,
  failed,
  forget,
  type Item,
  keepLater,
  left,
  readKept,
} from "./cache";
import { Failure, getAt } from "./request";
import { tryAgain } from "./state";

/** How many of the feed's events a page keeps, to apply again to an answer read while they came. */
export const KEPT_EVENTS = 128;
/** How many times running an answer the store could not place against the feed is read again. */
const AGAIN_MOST = 2;

/** What a feed event leaves of a part. */
export interface Heard<T> {
  /** Its value after the event. */
  value: T | undefined;
  /** The part is read again: the event names a change it does not carry. */
  again?: boolean;
  /** The server has no such thing any more, as its `404` says. */
  gone?: boolean;
}

/** A part of a page: the read a component draws, by its key. `E` is the page's feed event. */
export interface Part<T, E = unknown> {
  /** Unique on its page. */
  key: string;
  url: string;
  /** The answer as the part draws it; null for one this client cannot read, a failed read. */
  read(value: unknown): T | null;
  /** What a read whose answer this client cannot read fails with. */
  unreadable?: string;
  /** Where the answer is kept on this device, drawn at once on a return; nowhere without it. */
  kept?: { item: Item; part: string };
  /** Whether an answer says the server has no such thing, as a `404` does. */
  absent?(answer: T): boolean;
  /**
   * The value an answer leaves, given the one shown before it and whether the server answered
   * that one (`fresh`; a copy from this device otherwise): the answer itself by default.
   */
  merge?(answer: T, shown: T | undefined, fresh: boolean): T;
  /** What the feed's `event` leaves of `value`; null when it changes nothing of the part. */
  hear?(value: T | undefined, event: E): Heard<T> | null;
  /** The page's part that matters most: its first draw from the server is marked `first-part`. */
  first?: boolean;
}

/** What a part shows now; a new value at each change. */
export interface Shown<T> {
  readonly value: T | undefined;
  /** Drawn from this device, the server not having answered yet. */
  readonly stale: boolean;
  /** Why its last read failed; it is tried again by itself. */
  readonly error: Error | null;
  /**
   * A read is out since the last failed (the store's own try again, or a reader's ask), so a
   * control that asks for one says it reads. A part's first read, and one over an answer, set
   * nothing: its mock-up, or the answer shown, says it.
   */
  readonly retrying: boolean;
  /** The server has no such thing. */
  readonly missing: boolean;
  /** The server answered it, with a value or that it has none. */
  readonly answered: boolean;
}

const NOTHING: Shown<never> = {
  value: undefined,
  stale: false,
  error: null,
  retrying: false,
  missing: false,
  answered: false,
};

/** What the store tells the page that holds it. */
export interface PartsHost {
  /** What the page shows from this device changed. */
  freshness?(f: Freshness): void;
  /** The server answered the part `key`, and its answer is drawn. */
  answered?(key: string): void;
}

interface Entry<T, E> {
  part: Part<T, E>;
  shown: Shown<T>;
  /** The server's last answer, or this device's copy, as it came: a `304` shows it again. */
  base: T | undefined;
  etag: string;
  /** The feed's cursor of the last answer; none before one, or from a server that names none. */
  version: number | null;
  reading: Promise<void> | null;
  /** Asked again while a read was out, for an event of this version: read again after it unless
   * its answer is at that version or after. */
  newer: number | null;
  /** The store tries its failed read again by itself (`tryAgain`), until one answers. */
  trying: boolean;
  /** Ends when the page drops the part. */
  end: AbortController;
  signal: AbortSignal;
}

/** The error `thrown` is, as a page says it. */
function errorOf(thrown: unknown): Error {
  return thrown instanceof Error ? thrown : new Error(String(thrown));
}

/**
 * A page's store of parts, for as long as `signal` lasts: nothing it started goes on, and nothing
 * is drawn or kept, once it ends.
 */
export class Parts<E = unknown> {
  // biome-ignore lint/suspicious/noExplicitAny: each entry holds its own part's type, which its key stands for
  private readonly entries = new Map<string, Entry<any, E>>();
  private readonly listeners = new Set<() => void>();
  /** The feed's events kept, each with its count and its version. */
  private events: [number, number, E][] = [];
  private count = 0;
  /** The version the feed last opened from; none before it opened. */
  private floor: number | null = null;
  /** Settled once the read of the part opened last has been asked. */
  private asked: Promise<void> = Promise.resolve();
  fresh: Freshness = FRESH;

  constructor(
    readonly signal: AbortSignal,
    private readonly host: PartsHost = {},
  ) {}

  /** Tell `listener` of each change; what it returns stops it. */
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** What `part` shows now: nothing before it is opened. */
  shown<T>(part: Part<T, E>): Shown<T> {
    return (this.entries.get(part.key)?.shown as Shown<T> | undefined) ?? NOTHING;
  }

  /** Change what the page shows from this device (the feed's own state is the page's to say). */
  freshen(change: (f: Freshness) => Freshness): void {
    const next = change(this.fresh);
    if (next === this.fresh) return;
    this.fresh = next;
    this.host.freshness?.(next);
  }

  private gone(entry: Entry<unknown, E>): boolean {
    return entry.signal.aborted;
  }

  private show<T>(entry: Entry<T, E>, change: Partial<Shown<T>>): void {
    entry.shown = { ...entry.shown, ...change };
    for (const listener of [...this.listeners]) listener();
  }

  /**
   * Open `part`, once however many components name it: the copy this device kept of it drawn at
   * once, marked stale, then its read, naming that copy's tag. Reads are asked in the order their
   * parts were opened, whatever each looked up on this device first, so the parts a page opens
   * first (those on its first screen) are the first the server hears of.
   */
  open<T>(part: Part<T, E>): void {
    if (this.entries.has(part.key) || this.signal.aborted) return;
    const end = new AbortController();
    const entry: Entry<T, E> = {
      part,
      shown: NOTHING,
      base: undefined,
      etag: "",
      version: null,
      reading: null,
      newer: null,
      trying: false,
      end,
      signal: AbortSignal.any([this.signal, end.signal]),
    };
    this.entries.set(part.key, entry);
    const before = this.asked;
    let sent = () => {};
    this.asked = new Promise<void>((resolve) => {
      sent = resolve;
    });
    void this.start(entry, before, sent);
  }

  private async start<T>(
    entry: Entry<T, E>,
    before: Promise<void>,
    sent: () => void,
  ): Promise<void> {
    const { part } = entry;
    let reading: Promise<void> = Promise.resolve();
    try {
      await this.device(entry);
      if (this.gone(entry)) return;
      // The parts opened before this one are asked first: each waits its own device lookup only.
      await before;
      reading = this.read(part);
    } finally {
      sent();
    }
    await reading;
  }

  /** The copy this device kept of the part, drawn marked stale, unless the server answered it
   * while the device looked. Its tag is the one the part's next read names. */
  private async device<T>(entry: Entry<T, E>): Promise<void> {
    const { part } = entry;
    if (!part.kept) return;
    const copy = await readKept(part.kept.item, part.kept.part, part.read);
    if (this.gone(entry) || copy === null) return;
    if (entry.shown.answered || part.absent?.(copy.value)) return;
    entry.base = copy.value;
    entry.etag = copy.etag;
    const value = part.merge ? part.merge(copy.value, undefined, false) : copy.value;
    this.freshen((f) => drawn(f, part.key, copy.saved));
    this.show(entry, { value, stale: true });
  }

  /**
   * Read `part` again, for a feed event of version `newerThan` when one asked: one read a key, so
   * an ask while one is out joins it. A reader's own ask is served by that read's answer; an
   * event's has the part read once more after it if its answer is older. Settles once it is answered or
   * its read failed; a failed read is tried again by itself.
   */
  read<T>(part: Part<T, E>, newerThan?: number): Promise<void> {
    const entry = this.entries.get(part.key) as Entry<T, E> | undefined;
    if (entry === undefined || this.gone(entry)) return Promise.resolve();
    if (entry.reading !== null) {
      if (newerThan !== undefined) entry.newer = Math.max(entry.newer ?? 0, newerThan);
      return entry.reading;
    }
    const reading = this.reads(entry).finally(() => {
      entry.reading = null;
    });
    entry.reading = reading;
    return reading;
  }

  /** One read, and those it or an ask meanwhile calls for. */
  private async reads<T>(entry: Entry<T, E>): Promise<void> {
    for (let unplaced = 0; ; ) {
      entry.newer = null;
      // A read after one failed is said, until it answers or fails too.
      if (entry.shown.error !== null && !entry.shown.retrying) {
        this.show(entry, { retrying: true });
      }
      const placed = await this.once(entry);
      if (this.gone(entry) || entry.shown.error !== null) return;
      const asked = entry.newer;
      const older = asked !== null && (entry.version === null || asked > entry.version);
      if (placed) unplaced = 0;
      else unplaced += 1;
      if (!older && (placed || unplaced > AGAIN_MOST)) return;
    }
  }

  /**
   * The read failed: said, and tried again after each of the feed's waits until it answers. A part
   * with nothing drawn looks on this device again: the device's store goes on without a database
   * slow to open (shell/cache.ts), which may have opened since, and a server that does not answer
   * is when the copy is all the page has.
   */
  private fail<T>(entry: Entry<T, E>, thrown: unknown): void {
    const { part } = entry;
    this.freshen((f) => failed(f, part.key));
    this.show(entry, { error: errorOf(thrown), retrying: false });
    if (entry.shown.value === undefined) void this.device(entry);
    if (entry.trying) return;
    entry.trying = true;
    void tryAgain(
      () => !this.gone(entry) && entry.shown.error !== null,
      () => this.read(part),
      entry.signal,
    ).finally(() => {
      entry.trying = false;
    });
  }

  /** The server has no such thing: nothing of it is shown, and what this device kept goes. */
  private absent<T>(entry: Entry<T, E>): void {
    const { part } = entry;
    entry.base = undefined;
    entry.etag = "";
    entry.version = null;
    if (part.kept) void forget(part.kept.item);
    this.freshen((f) => answered(f, part.key));
    this.show(entry, {
      value: undefined,
      stale: false,
      error: null,
      retrying: false,
      missing: true,
      answered: true,
    });
    this.host.answered?.(part.key);
  }

  /**
   * One read of the part, drawn; false when the store could not place its answer against the feed
   * (it is read again), true otherwise, a failed read included.
   */
  private async once<T>(entry: Entry<T, E>): Promise<boolean> {
    const { part } = entry;
    const asked = this.count;
    const floor = this.floor;
    let base: T;
    let cursor: number | null;
    try {
      const got = await getAt(part.url, entry.etag, entry.signal);
      if (this.gone(entry)) return true;
      cursor = got.cursor;
      if (got.answer.same) {
        if (entry.base === undefined) throw new Error("The server answered 304 to a plain read");
        base = entry.base;
      } else {
        const read = part.read(got.answer.value);
        if (read === null)
          throw new Error(part.unreadable ?? "The server's answer is not readable");
        if (part.absent?.(read)) {
          this.absent(entry);
          return true;
        }
        base = read;
        entry.base = read;
        entry.etag = got.answer.etag;
        if (part.kept) {
          keepLater(part.kept.item, part.kept.part, got.answer.etag, got.answer.value);
        }
      }
    } catch (thrown) {
      if (this.gone(entry)) return true;
      if (thrown instanceof Failure && thrown.kind === "missing") this.absent(entry);
      else this.fail(entry, thrown);
      return true;
    }
    const first = !entry.shown.answered;
    entry.version = cursor;
    // What the feed said between the answer's cursor and the feed's own start, the store never
    // heard; nor what it dropped of what it did hear.
    const oldest = this.events[0];
    let placed =
      (cursor === null ? floor === this.floor : this.floor === null || cursor >= this.floor) &&
      (oldest === undefined || oldest[0] <= asked + 1);
    let value: T | undefined = part.merge
      ? part.merge(base, entry.shown.value, entry.shown.answered && !entry.shown.missing)
      : base;
    let gone = false;
    for (const [count, version, event] of this.events) {
      if (count <= asked || (cursor !== null && version <= cursor)) continue;
      const heard: Heard<T> | null = part.hear?.(value, event) ?? null;
      if (heard === null) continue;
      value = heard.value;
      gone ||= heard.gone === true;
      if (heard.again) placed = false;
    }
    if (gone) {
      this.absent(entry);
      return true;
    }
    this.freshen((f) => answered(f, part.key));
    this.show(entry, {
      value,
      stale: false,
      error: null,
      retrying: false,
      missing: false,
      answered: true,
    });
    if (first) {
      performance.mark(`part ${part.key}`);
      if (part.first) performance.mark("first-part");
    }
    this.host.answered?.(part.key);
    return placed;
  }

  /** The page's own change to what `part` shows (a page of its list read by the reader's ask). */
  change<T>(part: Part<T, E>, to: (value: T) => T): void {
    const entry = this.entries.get(part.key) as Entry<T, E> | undefined;
    if (entry === undefined || entry.shown.value === undefined) return;
    const value = to(entry.shown.value);
    if (value !== entry.shown.value) this.show(entry, { value });
  }

  /**
   * One event of the feed, of `version`: kept for the reads still out, and applied to each part it
   * is newer than (a part drawn from this device, or not answered, has no version yet).
   */
  heard(version: number, event: E): void {
    this.count += 1;
    this.events.push([this.count, version, event]);
    if (this.events.length > KEPT_EVENTS) this.events.shift();
    for (const entry of [...this.entries.values()]) {
      const { part } = entry;
      if (!part.hear || this.gone(entry)) continue;
      if (entry.version !== null && version <= entry.version) continue;
      const heard = part.hear(entry.shown.value, event);
      if (heard === null) continue;
      if (heard.gone) this.absent(entry);
      else if (heard.value !== entry.shown.value) this.show(entry, { value: heard.value });
      if (heard.again) void this.read(part, version);
    }
  }

  /**
   * The feed opened from `version`: a part answered before it (or at no version it named) is read
   * again, as the feed says nothing of what came between; one answered at or after it holds what
   * the feed will say from there, and one whose read is out is judged when it answers.
   */
  since(version: number): void {
    this.floor = version;
    for (const entry of [...this.entries.values()]) {
      const behind = entry.version === null || entry.version < version;
      if (entry.reading === null && entry.shown.answered && behind) {
        void this.read(entry.part);
      }
    }
  }

  /** The page shows the parts `keys` takes no more: their reads end, and none is drawn again. */
  drop(keys: (key: string) => boolean): void {
    const dropped: string[] = [];
    for (const [key, entry] of [...this.entries]) {
      if (!keys(key)) continue;
      entry.end.abort();
      this.entries.delete(key);
      dropped.push(key);
    }
    if (dropped.length === 0) return;
    this.freshen((f) => left(f, dropped));
    for (const listener of [...this.listeners]) listener();
  }
}

/** The store of the page a component is drawn in; none outside one (a component drawn alone). */
// biome-ignore lint/suspicious/noExplicitAny: a page's store takes its own event, which its parts name
export const PartsContext = createContext<Parts<any> | null>(null);

/**
 * What `part` shows, the component drawn again at each change of it: opened as the component
 * mounts, whoever else names it.
 */
export function usePart<T, E>(part: Part<T, E>): Shown<T> {
  const parts = useContext(PartsContext) as Parts<E> | null;
  const { key } = part;
  // biome-ignore lint/correctness/useExhaustiveDependencies: a part is its key: the store keeps the first of a key it was given
  const named = useCallback(
    (listener: () => void) => {
      if (parts === null) return () => {};
      parts.open(part);
      return parts.subscribe(listener);
    },
    [parts, key],
  );
  const shown = () => (parts === null ? NOTHING : parts.shown(part));
  return useSyncExternalStore(named, shown, shown);
}
