// The server's JSON API under the project's base: reads and the owner's posts, as the Dioxus
// client's `request.rs` sends them (a post is JSON, kept alive past the page).
import { href } from "./project.js";

/** The text a refusal or failure shows: the server's `error` when it sent one. */
async function failure(response) {
  try {
    const body = await response.json();
    if (body && typeof body.error === "string") return body.error;
  } catch {}
  return `${response.status} ${response.statusText}`.trim();
}

/** GET `path` as JSON; throws an Error naming why it failed. */
export async function get(path, { signal } = {}) {
  const response = await fetch(href(path), { signal });
  if (!response.ok) throw new Error(await failure(response));
  return response.json();
}

/** `get_found`: GET `path` as JSON, or `null` when the server has no such thing (404). */
export async function getFound(path, { signal } = {}) {
  const response = await fetch(href(path), { signal });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(await failure(response));
  return response.json();
}

/** POST `fields` to `path` as JSON; resolves to the server's answer. */
export async function post(path, fields) {
  const response = await fetch(href(path), {
    method: "POST",
    keepalive: true,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(fields),
  });
  if (!response.ok) throw new Error(await failure(response));
  const text = await response.text();
  return text ? JSON.parse(text) : null;
}

/** A query value, encoded. */
export const encode = encodeURIComponent;

/** `&cursor=CURSOR` for a page after `cursor`, nothing for the first. */
export function cursorQuery(cursor) {
  return cursor ? `&cursor=${encode(cursor)}` : "";
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
