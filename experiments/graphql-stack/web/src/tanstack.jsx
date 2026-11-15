// TanStack Query over the REST reads as they are: no GraphQL, a cache keyed by the read.
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { Column, mount } from "./shared";

const client = new QueryClient();
const json = (path) => fetch(path).then((r) => r.json());

function Page() {
  const board = useQuery({ queryKey: ["board"], queryFn: () => json("/api/board") });
  const column = useQuery({
    queryKey: ["task-board", "ready"],
    queryFn: () => json("/api/task-board?status=ready"),
  });
  const data = board.data && column.data ? { board: board.data, column: column.data.tasks } : null;
  return <Column data={data} error={board.error ?? column.error} />;
}

mount(
  <QueryClientProvider client={client}>
    <Page />
  </QueryClientProvider>,
);
