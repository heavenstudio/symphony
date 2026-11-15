// TanStack Query over the reads as they are: a query per key, its queryFn the client's own read
// (the kept copy's tag named, a 304 giving the copy back), the device's copy as placeholder data,
// and the feed setting a pushed value or invalidating a named key. In memory only.
import { QueryClient, QueryClientProvider, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
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

const client = new QueryClient();
const tags = new Map();

async function read(key) {
  const answer = await getIf(key, tags.get(key)?.etag);
  if (answer.same) return tags.get(key).value;
  tags.set(key, answer);
  keepLater(key, answer.etag, answer.value);
  return answer.value;
}

/** The device's copy of `key`, read before the query starts, so its read names the copy's tag. */
function useDevice(key) {
  const [kept, setKept] = useState(undefined);
  useEffect(() => {
    readKept(key).then((copy) => {
      if (copy) tags.set(key, copy);
      setKept(copy);
    });
  }, [key]);
  return kept;
}

function useKept(key) {
  const kept = useDevice(key);
  const query = useQuery({
    queryKey: [key],
    queryFn: () => read(key),
    enabled: kept !== undefined,
    placeholderData: kept?.value,
  });
  return { value: query.data, stale: query.isPlaceholderData, error: query.error };
}

function Feed() {
  const queries = useQueryClient();
  useEffect(
    () =>
      openFeed((event) => {
        for (const change of event.changes) {
          if ("value" in change) queries.setQueryData([change.key], change.value);
          else void queries.invalidateQueries({ queryKey: [change.key] });
        }
      }),
    [queries],
  );
  return null;
}

function CountsPart() {
  const { value, stale } = useKept(COUNTS);
  return <Counts data={value} stale={stale} />;
}

function ColumnPart() {
  const { value, stale, error } = useKept(COLUMN);
  return <Column data={value} stale={stale} error={value ? null : error} />;
}

mount(
  <QueryClientProvider client={client}>
    <Feed />
    <CountsPart />
    <ColumnPart />
  </QueryClientProvider>,
);
