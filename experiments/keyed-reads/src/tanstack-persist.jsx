// TanStack Query with its own persistence in place of the client's device cache: the whole query
// cache written to the shell's store as one value and restored before any query runs. A restored
// query has no tag to name, so its read downloads the answer whole (no 304).
import { QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";
import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { useEffect } from "react";
import { COLUMN, COUNTS, Column, Counts, mount, openFeed } from "./shared";

const client = new QueryClient({ defaultOptions: { queries: { gcTime: 14 * 86_400_000 } } });

/** The persister over the shell's store: one value for the whole cache. */
const persister = {
  persistClient: (state) => globalThis.symphonyCache?.keep("queries", "", state),
  restoreClient: async () => (await globalThis.symphonyCache?.read("queries"))?.value,
  removeClient: () => globalThis.symphonyCache?.forget("queries"),
};

const json = (path) => fetch(path).then((response) => response.json());

function useKept(key) {
  const query = useQuery({ queryKey: [key], queryFn: () => json(key) });
  return { value: query.data, stale: query.isStale && query.isFetching, error: query.error };
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
  <PersistQueryClientProvider client={client} persistOptions={{ persister }}>
    <Feed />
    <CountsPart />
    <ColumnPart />
  </PersistQueryClientProvider>,
);
