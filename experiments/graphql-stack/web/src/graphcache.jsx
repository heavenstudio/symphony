// urql with Graphcache, its normalized cache: each task kept once by its key, every query that
// names it answered from that one record.
import { cacheExchange } from "@urql/exchange-graphcache";
import { Client, fetchExchange, Provider, useQuery } from "urql";
import { Column, mount, QUERY } from "./shared";

const client = new Client({
  url: "/graphql",
  exchanges: [
    cacheExchange({ keys: { Task: (task) => task.slug, Board: () => null, Counts: () => null } }),
    fetchExchange,
  ],
});

function Page() {
  const [{ data, error }] = useQuery({ query: QUERY, variables: { status: "ready" } });
  return <Column data={data} error={error} />;
}

mount(
  <Provider value={client}>
    <Page />
  </Provider>,
);
