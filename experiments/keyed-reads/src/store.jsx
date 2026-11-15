// The client's own way, extended: one keyed store. A component names the key it draws; the store
// draws the device's copy first, reads the key once however many components name it, names the kept
// copy's tag, tries a failed read again, and takes a value the feed pushes. No dependency.
import { useSyncExternalStore } from "react";
import {
  COLUMN,
  COUNTS,
  Column,
  Counts,
  getIf,
  keepLater,
  mount,
  openFeed,
  readKept,
} from "./shared";

/** key → what it shows (`{ value, stale, error }`), its tag, its read while one is out, and who
 * draws it. */
const entries = new Map();

function entry(key) {
  let found = entries.get(key);
  if (!found) {
    found = {
      shown: { value: undefined, stale: false, error: null },
      etag: "",
      reading: null,
      listeners: new Set(),
    };
    entries.set(key, found);
  }
  return found;
}

function show(key, change) {
  const held = entry(key);
  held.shown = { ...held.shown, ...change };
  for (const listener of held.listeners) listener();
}

/** A value from the server, a read's answer or the feed's push: drawn, and kept on the device. */
export function put(key, value, etag = "") {
  entry(key).etag = etag;
  if (etag) keepLater(key, etag, value);
  show(key, { value, stale: false, error: null });
}

/** Read `key` unless a read of it is out; a failure is tried again, each wait twice the last. */
export function read(key, wait = 1_000) {
  const held = entry(key);
  if (held.reading) return held.reading;
  held.reading = getIf(key, held.etag)
    .then((answer) => {
      held.reading = null;
      if (answer.same) show(key, { stale: false, error: null });
      else put(key, answer.value, answer.etag);
    })
    .catch((error) => {
      held.reading = null;
      show(key, { error });
      if (held.listeners.size > 0) setTimeout(() => read(key, Math.min(wait * 2, 30_000)), wait);
    });
  return held.reading;
}

/** The first component to name `key` draws the device's copy and starts its read. */
function subscribe(key, listener) {
  const held = entry(key);
  held.listeners.add(listener);
  if (held.listeners.size === 1 && held.shown.value === undefined) {
    readKept(key).then((kept) => {
      if (kept && held.shown.value === undefined) {
        held.etag = kept.etag;
        show(key, { value: kept.value, stale: true });
      }
      void read(key);
    });
  }
  return () => held.listeners.delete(listener);
}

/** What `key` shows now: `{ value, stale, error }`, a new object at each change. */
export function useKept(key) {
  return useSyncExternalStore(
    (listener) => subscribe(key, listener),
    () => entry(key).shown,
  );
}

/** The feed: a key pushed with its value is drawn at once; one only named is read again. */
openFeed((event) => {
  for (const change of event.changes) {
    if (!entries.has(change.key)) continue;
    if ("value" in change) put(change.key, change.value, change.etag);
    else void read(change.key);
  }
});

function CountsPart() {
  const { value, stale } = useKept(COUNTS);
  return <Counts data={value} stale={stale} />;
}

function ColumnPart() {
  const { value, stale, error } = useKept(COLUMN);
  return <Column data={value} stale={stale} error={value ? null : error} />;
}

mount(
  <>
    <CountsPart />
    <ColumnPart />
  </>,
);
