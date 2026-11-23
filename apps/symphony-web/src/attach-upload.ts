// What a composer's files are sent with (attach.tsx), a chunk of its own: imported once a composer
// is given its first file, so a page whose owner attaches nothing carries none of it. Each file is
// sent to UPLOAD_PATH as it comes, one at a time, and held as its chip once the server stored it.
// It imports nothing of a kind's own: attach.tsx is the composers' shared chunk.

import { type Attachments, MAX_FILES } from "./attach";
import { here, project } from "./project";
import { deadline } from "./timing";
import { UPLOAD_MAX_BYTES, UPLOAD_PATH, type Upload } from "./wire";
import { fixed } from "./words";

/** How long an upload waits for the server: a phone on a slow link sends 10 MB well within it. */
export const UPLOAD_MS = 120_000;

/** A file bigger than the server takes, said before any byte is sent. */
export function tooLarge(name: string, bytes: number): string | null {
  if (bytes <= UPLOAD_MAX_BYTES) return null;
  const megabytes = UPLOAD_MAX_BYTES / (1024 * 1024);
  return `${name} is ${fixed(bytes / (1024 * 1024), 1)} MB; uploads are at most ${Math.floor(megabytes)} MB`;
}

/** Whether `value` is an upload as the server answers it. */
export function isUpload(value: unknown): value is Upload {
  if (typeof value !== "object" || value === null) return false;
  const { url, name, picture, bytes, markup } = value as Record<string, unknown>;
  return (
    typeof url === "string" &&
    typeof name === "string" &&
    typeof picture === "boolean" &&
    typeof bytes === "number" &&
    typeof markup === "string"
  );
}

/**
 * Send one file as the raw body; the server answers its stored line, or why it refused. Aborted,
 * and said so, when no answer came in UPLOAD_MS.
 */
export async function upload(file: File): Promise<Upload> {
  const controller = new AbortController();
  const ended = deadline(UPLOAD_MS, () => controller.abort());
  try {
    const url = `${here(project(), UPLOAD_PATH)}?name=${encodeURIComponent(file.name)}`;
    let response: Response;
    try {
      response = await fetch(url, {
        method: "POST",
        signal: controller.signal,
        body: file,
        headers: { "Content-Type": file.type === "" ? "application/octet-stream" : file.type },
      });
    } catch (error) {
      if (controller.signal.aborted) throw new Error("no answer from the server; try again");
      throw error;
    }
    let value: unknown;
    try {
      value = await response.json();
    } catch {
      if (controller.signal.aborted) throw new Error("no answer from the server; try again");
      throw new Error(`the server answered ${response.status}`);
    }
    if (!response.ok) {
      const said = (value as { error?: unknown } | null)?.error;
      throw new Error(typeof said === "string" ? said : `the server answered ${response.status}`);
    }
    if (!isUpload(value)) throw new Error("the server's answer is no upload");
    return value;
  } finally {
    ended();
  }
}

/**
 * The files of one batch taken in turn, one upload at a time, into `held` (`changed` after each
 * change): past MAX_FILES the rest are refused at once, and a file past the server's size is
 * named and skipped. A file the server already holds under its address is held once. `kept` saves
 * the files held; `alive` says whether the composer is still there to take an answer.
 */
export async function take(
  batch: readonly File[],
  held: Attachments,
  changed: () => void,
  kept: () => void,
  alive: () => boolean,
  send: (file: File) => Promise<Upload> = upload,
): Promise<void> {
  for (const file of batch) {
    const name = file.name;
    if (held.files.length + held.pending.length >= MAX_FILES) {
      held.trouble = `At most ${MAX_FILES} files per post`;
      changed();
      break;
    }
    const large = tooLarge(name, file.size);
    if (large !== null) {
      held.trouble = large;
      changed();
      continue;
    }
    held.pending = [...held.pending, name];
    changed();
    let stored: Upload | null = null;
    let why = "";
    try {
      stored = await send(file);
    } catch (error) {
      why = error instanceof Error ? error.message : String(error);
    }
    if (!alive()) return;
    const at = held.pending.indexOf(name);
    if (at >= 0) held.pending = held.pending.filter((_, one) => one !== at);
    if (stored !== null) {
      const added = stored;
      if (!held.files.some((one) => one.url === added.url)) held.files = [...held.files, added];
      held.trouble = null;
      kept();
    } else {
      held.trouble = `${name}: ${why}`;
    }
    changed();
  }
}
