// SWR over the reads as they are: a hook per key, its fetcher the client's own read (the kept
// copy's tag named), the device's copy as fallback data, and the feed mutating a pushed value or
// revalidating a named key. In memory only.
import { useEffect, useState } from "react";
import useSWR, { mutate } from "swr";
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

async function read(key) {
  const answer = await getIf(key, tags.get(key)?.etag);
  if (answer.same) return tags.get(key).value;
  tags.set(key, answer);
  keepLater(key, answer.etag, answer.value);
  return answer.value;
}

function useKept(key) {
  const [kept, setKept] = useState(undefined);
  useEffect(() => {
    readKept(key).then((copy) => {
      if (copy) tags.set(key, copy);
      setKept(copy);
    });
  }, [key]);
  const { data, error } = useSWR(kept === undefined ? null : key, read, {
    fallbackData: kept?.value,
    revalidateOnFocus: false,
  });
  return { value: data, stale: data !== undefined && data === kept?.value, error };
}

openFeed((event) => {
  for (const change of event.changes) {
    if ("value" in change) void mutate(change.key, change.value, { revalidate: false });
    else void mutate(change.key);
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
