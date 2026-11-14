// urql with its default document cache (answers kept by query and variables, dropped when a
// mutation names their types).
import { cacheExchange, Client, fetchExchange, Provider, useQuery } from "urql";
import { Column, mount, QUERY } from "./shared";

const client = new Client({ url: "/graphql", exchanges: [cacheExchange, fetchExchange] });

function Page() {
  const [{ data, error }] = useQuery({ query: QUERY, variables: { status: "ready" } });
  return <Column data={data} error={error} />;
}

mount(
  <Provider value={client}>
    <Page />
  </Provider>,
);
