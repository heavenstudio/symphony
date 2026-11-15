// React's own: each component reads a promise with `use()` under a Suspense boundary of its own,
// and a transition swaps a fresh promise in. No store and no dependency; the device's copy, the tag
// and the feed are joined by hand, as they are in the keyed store.
import { Suspense, startTransition, use, useEffect, useState } from "react";
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

const tags = new Map();

/** The device's copy when it has one (marked stale), else the server's answer. */
async function first(key) {
  const kept = await readKept(key);
  if (kept) {
    tags.set(key, kept.etag);
    return { value: kept.value, stale: true };
  }
  return fresh(key, undefined);
}

async function fresh(key, shown) {
  const answer = await getIf(key, tags.get(key));
  if (answer.same) return { value: shown, stale: false };
  tags.set(key, answer.etag);
  keepLater(key, answer.etag, answer.value);
  return { value: answer.value, stale: false };
}

const listeners = new Map();
/** A key's first promise, kept outside the component: React keeps no state of a render that
 * suspended before it mounted, so one made in the render would be made again at every try. */
const firsts = new Map();

function firstOf(key) {
  if (!firsts.has(key)) firsts.set(key, first(key));
  return firsts.get(key);
}

function Part({ name, draw }) {
  const [promise, setPromise] = useState(() => firstOf(name));
  const shown = use(promise);
  useEffect(() => {
    const again = () => startTransition(() => setPromise(fresh(name, shown.value)));
    listeners.set(name, again);
    if (shown.stale) again();
    return () => listeners.delete(name);
  }, [name, shown]);
  return draw(shown);
}

openFeed((event) => {
  for (const change of event.changes) listeners.get(change.key)?.();
});

mount(
  <>
    <Suspense fallback={<Counts />}>
      <Part name={COUNTS} draw={({ value, stale }) => <Counts data={value} stale={stale} />} />
    </Suspense>
    <Suspense fallback={<Column />}>
      <Part name={COLUMN} draw={({ value, stale }) => <Column data={value} stale={stale} />} />
    </Suspense>
  </>,
);
