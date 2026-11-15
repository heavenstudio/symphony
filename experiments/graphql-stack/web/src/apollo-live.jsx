// apollo.jsx with live updates as GraphQL subscriptions over a WebSocket (graphql-ws), in place of
// today's Server-Sent Events feed: the query over HTTP, the subscription over the socket.
import { ApolloClient, gql, HttpLink, InMemoryCache, split } from "@apollo/client";
import { GraphQLWsLink } from "@apollo/client/link/subscriptions";
import { ApolloProvider, useQuery, useSubscription } from "@apollo/client/react";
import { getMainDefinition } from "@apollo/client/utilities";
import { createClient } from "graphql-ws";
import { Column, mount, QUERY, SUBSCRIPTION } from "./shared";

const socket = new GraphQLWsLink(createClient({ url: `ws://${location.host}/graphql/ws` }));
const link = split(
  ({ query }) => {
    const definition = getMainDefinition(query);
    return definition.kind === "OperationDefinition" && definition.operation === "subscription";
  },
  socket,
  new HttpLink({ uri: "/graphql" }),
);
const client = new ApolloClient({
  link,
  cache: new InMemoryCache({ typePolicies: { Task: { keyFields: ["slug"] } } }),
});
const query = gql(QUERY);
const feed = gql(SUBSCRIPTION);

function Page() {
  const { data, error, refetch } = useQuery(query, {
    variables: { status: "ready" },
    fetchPolicy: "cache-and-network",
  });
  useSubscription(feed, {
    variables: { since: data?.board.version ?? 0 },
    skip: !data,
    onData: () => refetch(),
  });
  return <Column data={data} error={error} />;
}

mount(
  <ApolloProvider client={client}>
    <Page />
  </ApolloProvider>,
);
