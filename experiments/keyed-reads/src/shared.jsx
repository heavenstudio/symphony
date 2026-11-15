// What every page draws: two components, the board's counts and one column's cards, each from a
// read of its own, and what every page shares with the client as it is: the read that names the
// kept copy's tag, the device's store (the shell's `symphonyCache`) and the feed's socket. The same
// lines in every build, so a build's bytes beyond the baseline's are its way of joining them.
import { createRoot } from "react-dom/client";

export const COUNTS = "/api/task-board?counts=1";
export const COLUMN = "/api/task-board?status=ready";

/** A read naming the kept copy's tag: `same` for a 304, else the value and its tag. */
export async function getIf(path, etag) {
  const response = await fetch(path, etag ? { headers: { "If-None-Match": etag } } : {});
  if (response.status === 304) return { same: true };
  if (!response.ok) throw new Error(`The server answered ${response.status}`);
  return { same: false, value: await response.json(), etag: response.headers.get("ETag") ?? "" };
}

/** What this device kept under `key`: `{ value, etag, saved }`, or none. */
export async function readKept(key) {
  return (await globalThis.symphonyCache?.read(key)) ?? null;
}

export function keepLater(key, etag, value) {
  void globalThis.symphonyCache?.keep(key, etag, value);
}

/** The feed: each event names the keys that moved, some with their new value. */
export function openFeed(heard) {
  const socket = new WebSocket(`ws://${location.host}/api/feed`);
  socket.addEventListener("message", (event) => heard(JSON.parse(event.data)));
  return () => socket.close();
}

export function Counts({ data, stale }) {
  if (!data) return <p aria-busy="true">Loading</p>;
  return <output data-cache={stale ? "device" : undefined}>{data.counts.ready}</output>;
}

export function Column({ data, stale, error }) {
  if (error) return <p role="alert">{String(error.message ?? error)}</p>;
  if (!data) return <p aria-busy="true">Loading</p>;
  return (
    <ol data-cache={stale ? "device" : undefined}>
      {data.tasks.map((card) => (
        <li key={card.slug} data-priority={card.priority}>
          {card.title}
        </li>
      ))}
    </ol>
  );
}

export function mount(page) {
  createRoot(document.getElementById("root")).render(page);
}
