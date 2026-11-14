# GraphQL's price, measured

A probe, not a client or a server: what a GraphQL layer would cost the Symphony, against what it
has. What it found is `docs/analysis/graphql-and-server-framework.md`. Nothing the Symphony
builds, tests, ships or serves reads this folder.

## The client's bytes (`web/`)

One React page per data library, each drawing the same board and column from one read: the web
client's way today (a hand-written `fetch`, `src/baseline.jsx`), TanStack Query over the REST
reads, graphql-request, urql, urql with Graphcache, Apollo Client, Apollo with subscriptions over
graphql-ws, and Relay (its compiler's artifact written out by hand). Each is built alone by Vite
with the web client's settings, and its script gzipped at the best level, as the server sends it:

```sh
cd web
npm ci
npm run measure
```

The pages are built, not run: no page here asks a server anything.

## The server's cost (`server/`)

The same read on axum 0.8 three ways, one Cargo feature each: a plain JSON read (`rest`),
async-graphql and Juniper. `measure.sh` compiles each from nothing in a target folder of its own,
release as the shipped server is, and prints its CPU and wall time with the machine's load, its
stripped binary's size, the time a read and a query take in the process (medians of 15 batches
of 1,000, the plain read and the query in turn) and one query answered on a private loopback port:

```sh
server/measure.sh
```
