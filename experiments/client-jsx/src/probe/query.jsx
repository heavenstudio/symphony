// TanStack Query for the pages' reads: the board's counts and a column's pages with Load more (an
// infinite query, as a task's thread pages too), and a feed event answered by invalidating what it
// touched, so that every read in view is asked again and drawn. The server is a stand-in in the
// page, answering after 20 ms, so the probe needs no Symphony server; what it tests is the
// library's store reaching the components, which runs through `useSyncExternalStore`.
import { QueryClient, QueryClientProvider, useInfiniteQuery, useIsFetching, useQuery, useQueryClient } from "@tanstack/react-query";

const PAGE = 30;
const server = { reads: 0, ready: 95, suffix: "" };

function answer(value) {
  return new Promise((resolve) =>
    setTimeout(() => {
      server.reads += 1;
      resolve(value());
    }, 20),
  );
}

const counts = () => answer(() => ({ ready: server.ready }));

const page = (cursor) =>
  answer(() => ({
    cards: Array.from({ length: Math.max(0, Math.min(PAGE, server.ready - cursor)) }, (_, i) => `task ${cursor + i}${server.suffix}`),
    next: cursor + PAGE < server.ready ? cursor + PAGE : null,
  }));

function Column() {
  const client = useQueryClient();
  const total = useQuery({ queryKey: ["counts"], queryFn: counts });
  const column = useInfiniteQuery({
    queryKey: ["column", "ready"],
    queryFn: ({ pageParam }) => page(pageParam),
    initialPageParam: 0,
    getNextPageParam: (last) => last.next,
  });
  const fetching = useIsFetching();
  const cards = column.data?.pages.flatMap((one) => one.cards) ?? [];
  // A task added and every card renamed, as the feed would report them.
  const event = () => {
    server.ready += 1;
    server.suffix = " (renamed)";
    client.invalidateQueries();
  };
  return (
    <section id="query" data-ready={String(total.isSuccess && column.isSuccess)} data-fetching={fetching}>
      <output id="query-total">{total.data?.ready ?? ""}</output>
      <ol id="query-cards">
        {cards.map((card) => (
          <li key={card}>{card}</li>
        ))}
      </ol>
      <button id="query-more" type="button" disabled={!column.hasNextPage || column.isFetchingNextPage} onClick={() => column.fetchNextPage()}>
        Load more
      </button>
      <button id="query-event" type="button" onClick={event}>
        A feed event
      </button>
    </section>
  );
}

const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });

export default function Query() {
  return (
    <QueryClientProvider client={client}>
      <Column />
    </QueryClientProvider>
  );
}
