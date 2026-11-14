// Relay: its runtime, normalized store and React bindings. A real client runs Relay's compiler,
// which writes each query's artifact; this is that artifact for QUERY, written out as the compiler
// writes it, so no compiler is needed to count the runtime's bytes.
import { Suspense } from "react";
import { RelayEnvironmentProvider, useLazyLoadQuery } from "react-relay";
import { Environment, Network, RecordSource, Store } from "relay-runtime";
import { Column, mount, QUERY } from "./shared";

const scalar = (name) => ({ alias: null, args: null, kind: "ScalarField", name, storageKey: null });
const selections = [
  {
    alias: null,
    args: null,
    concreteType: "Board",
    kind: "LinkedField",
    name: "board",
    plural: false,
    selections: [
      scalar("version"),
      {
        alias: null,
        args: null,
        concreteType: "Counts",
        kind: "LinkedField",
        name: "counts",
        plural: false,
        selections: [scalar("ready"), scalar("inProgress"), scalar("review")],
        storageKey: null,
      },
    ],
    storageKey: null,
  },
  {
    alias: null,
    args: [{ kind: "Variable", name: "status", variableName: "status" }],
    concreteType: "Task",
    kind: "LinkedField",
    name: "column",
    plural: true,
    selections: [scalar("slug"), scalar("title"), scalar("priority")],
    storageKey: null,
  },
];
const argumentDefinitions = [{ defaultValue: null, kind: "LocalArgument", name: "status" }];
const ColumnQuery = {
  fragment: {
    argumentDefinitions,
    kind: "Fragment",
    metadata: null,
    name: "ColumnQuery",
    selections,
    type: "Query",
    abstractKey: null,
  },
  kind: "Request",
  operation: { argumentDefinitions, kind: "Operation", name: "ColumnQuery", selections },
  params: {
    cacheID: "column-query",
    id: null,
    metadata: {},
    name: "ColumnQuery",
    operationKind: "query",
    text: QUERY,
  },
};

const environment = new Environment({
  network: Network.create((params, variables) =>
    fetch("/graphql", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: params.text, variables }),
    }).then((r) => r.json()),
  ),
  store: new Store(new RecordSource()),
});

function Page() {
  const data = useLazyLoadQuery(ColumnQuery, { status: "ready" });
  return <Column data={data} />;
}

mount(
  <RelayEnvironmentProvider environment={environment}>
    <Suspense fallback={<Column />}>
      <Page />
    </Suspense>
  </RelayEnvironmentProvider>,
);
