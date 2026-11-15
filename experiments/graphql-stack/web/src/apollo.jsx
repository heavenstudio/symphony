// Apollo Client 4 with its normalized InMemoryCache over HTTP: the owner's named option.
import { ApolloClient, gql, HttpLink, InMemoryCache } from "@apollo/client";
import { ApolloProvider, useQuery } from "@apollo/client/react";
import { Column, mount, QUERY } from "./shared";

const client = new ApolloClient({
  link: new HttpLink({ uri: "/graphql" }),
  cache: new InMemoryCache({ typePolicies: { Task: { keyFields: ["slug"] } } }),
});
const query = gql(QUERY);

function Page() {
  const { data, error } = useQuery(query, {
    variables: { status: "ready" },
    fetchPolicy: "cache-and-network",
  });
  return <Column data={data} error={error} />;
}

mount(
  <ApolloProvider client={client}>
    <Page />
  </ApolloProvider>,
);
