// What every page draws: the board's counts and one column's cards, from whichever library read
// them. The same few lines in every build, so a build's bytes beyond the baseline's are its
// library's and the lines that set it up.
import { createRoot } from "react-dom/client";

export const QUERY = `query Column($status: String!) {
  board { version counts { ready inProgress review } }
  column(status: $status) { slug title priority }
}`;

export const SUBSCRIPTION = `subscription Feed($since: Int!) {
  feed(since: $since) { version changes }
}`;

export function Column({ data, error }) {
  if (error) return <p role="alert">{String(error.message ?? error)}</p>;
  if (!data) return <p aria-busy="true">Loading</p>;
  return (
    <section data-version={data.board.version}>
      <output>{data.board.counts.ready}</output>
      <ol>
        {data.column.map((card) => (
          <li key={card.slug} data-priority={card.priority}>
            {card.title}
          </li>
        ))}
      </ol>
    </section>
  );
}

export function mount(page) {
  createRoot(document.getElementById("root")).render(page);
}
