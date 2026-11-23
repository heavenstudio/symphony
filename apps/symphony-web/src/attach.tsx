// One attach control for every place the owner writes on a document page: a card's message,
// verdict note or comment, a question and its follow-ups. The Dioxus client's attach.rs, which the
// panel's chat and new task and a task's thread still draw: the same rows on this device, so a
// draft written on one client reads back on the other, and the same style element.
//
// A composer takes `useAttach` with a key of its own, marks its form `data-attach` with its `zone`
// and draws `AttachControls` inside it. A file reaches it three ways: the picker, a paste (a
// screenshot on the clipboard) or a drop anywhere on the form. Each is sent to UPLOAD_PATH at once,
// one at a time, by attach-upload.ts (a chunk imported as the first file comes), and shown as a
// chip; the post carries one line per file (`compose`), which is how the posted item keeps showing
// it. The chips are saved with the draft, so a reload keeps them.
// One set of document listeners routes paste, drop and the picker's change to the composer whose
// zone holds the event's target.

import { useEffect, useReducer, useRef, useState } from "react";
import type { take } from "./attach-upload";
import { Button } from "./button";
import { snapshot } from "./shared";
import { load, save } from "./storage";
import STYLE from "./styles/attach.css?inline";
import { UPLOAD_ACCEPT, type Upload } from "./wire";

/** The most files one post carries. */
export const MAX_FILES = 10;

/** Where this device keeps a composer's files, beside its draft. */
export function storageKey(key: string): string {
  return `review-attach:${key}`;
}

/**
 * The draft's files as the browser keeps them: one `url\tname\tpicture\tbytes\tmarkup` row each,
 * as the Dioxus client keeps them.
 */
export function saved(files: readonly Upload[]): string {
  return files
    .map((file) => `${file.url}\t${file.name}\t${file.picture}\t${file.bytes}\t${file.markup}`)
    .join("\n");
}

/** A whole number as Rust's `u64` parses it: digits, an optional `+` first. */
function count(text: string): number | null {
  if (!/^\+?[0-9]+$/.test(text)) return null;
  const value = Number(text);
  return Number.isSafeInteger(value) ? value : null;
}

/** The files `text` keeps (`saved`); a row that does not read is left out. */
export function restored(text: string): Upload[] {
  const files: Upload[] = [];
  for (const line of text.split(/\r?\n/)) {
    const [url, name, picture, bytes, ...markup] = line.split("\t");
    const size = count(bytes ?? "");
    if (url === undefined || name === undefined || picture === undefined || size === null) {
      continue;
    }
    if (markup.length === 0) continue;
    files.push({ url, name, picture: picture === "true", bytes: size, markup: markup[0] ?? "" });
  }
  return files;
}

/**
 * `text` followed, after a blank line, by one line per file: the line the server gave each upload.
 * Text with no files is sent as it was typed.
 */
export function compose(text: string, files: readonly Upload[]): string {
  if (files.length === 0) return text;
  const lines = files.map((file) => file.markup).join("\n");
  const typed = text.trimEnd();
  return typed.trim() === "" ? lines : `${typed}\n\n${lines}`;
}

/** A composer's attachments: the files stored so far, the ones on their way, the last refusal. */
export interface Attachments {
  files: Upload[];
  pending: string[];
  trouble: string | null;
}

/** A mounted composer: its mount's id and what takes its files. */
interface Zone {
  id: number;
  take(files: File[]): void;
}

const zones = new Map<string, Zone>();
let next = 0;
const EVENTS = ["paste", "dragover", "dragleave", "drop", "change"] as const;

/** The files of a list, or none. */
function filesOf(list: FileList | null | undefined): File[] {
  return list ? Array.from(list) : [];
}

/**
 * One event of the page's: a paste, a drag or a drop on a composer's form, or its picker's change,
 * handed to the composer whose zone (`data-attach`) holds its target, unless the zone's controls
 * are off while it sends. A pasted picture is not text for the box, and a dropped file must not
 * open in the tab.
 */
export function routed(event: Event, found: ReadonlyMap<string, Zone> = zones): void {
  const target = event.target as Element | null;
  if (typeof target?.closest !== "function") return;
  const zone = target.closest("[data-attach]");
  const key = zone?.getAttribute("data-attach");
  if (!zone || key == null) return;
  if (zone.querySelector("[data-attach-off=true]")) return;
  const owner = found.get(key);
  if (!owner) return;
  let files: File[];
  switch (event.type) {
    case "paste":
      files = filesOf((event as ClipboardEvent).clipboardData?.files);
      break;
    case "dragover": {
      const types = (event as DragEvent).dataTransfer?.types ?? [];
      if (Array.from(types).includes("Files")) {
        event.preventDefault();
        zone.setAttribute("data-dropping", "true");
      }
      return;
    }
    case "dragleave":
      zone.removeAttribute("data-dropping");
      return;
    case "drop":
      zone.removeAttribute("data-dropping");
      files = filesOf((event as DragEvent).dataTransfer?.files);
      break;
    case "change": {
      const input = target as HTMLInputElement;
      if (!input.hasAttribute("data-attach-input")) return;
      files = filesOf(input.files);
      // The same file picked again must fire again.
      input.value = "";
      break;
    }
    default:
      return;
  }
  if (files.length === 0) return;
  event.preventDefault();
  owner.take(files);
}

function listen(on: boolean): void {
  for (const name of EVENTS) {
    if (on) document.addEventListener(name, routed as EventListener);
    else document.removeEventListener(name, routed as EventListener);
  }
}

/** Put the control's style in the page once: the panel's module and this page share it. */
function style(): void {
  const id = "attach-style";
  if (document.getElementById(id)) return;
  const element = document.createElement("style");
  element.id = id;
  element.textContent = STYLE;
  document.head.append(element);
}

/** A composer's attachments, as its form and its post use them. */
export interface Attach {
  /** The `data-attach` value of the composer's form: where a paste or a drop lands. */
  zone: string;
  files: Upload[];
  pending: string[];
  trouble: string | null;
  /** Whether a file is still on its way: the post waits for it. */
  busy: boolean;
  empty: boolean;
  /** `text` with a line for each attached file. */
  compose(text: string): string;
  /** After a post: the files went with it. */
  clear(): void;
  remove(url: string): void;
}

/** The files' sending (attach-upload.ts), imported as a composer is given its first file. */
const sending = () => import("./attach-upload");

/**
 * What a composer says where the files' sending did not come, once the shell said the page's build
 * is there (where it is gone, the page loads again): the files are not taken, and are given again.
 */
export const UNSENT =
  "The files could not be sent: this page could not load its uploads. Try again.";

/**
 * A composer's `files`, taken into `held` after the batches `before` them (attach-upload.ts `take`,
 * whose `changed`, `kept` and `alive` these are) once their sending has come. Where it did not come,
 * UNSENT is said, unless the composer left or the page's build is gone (the page then loads again).
 */
export function sendAfter(
  before: Promise<void>,
  files: File[],
  held: Attachments,
  changed: () => void,
  kept: () => void,
  alive: () => boolean,
  load: () => Promise<{ take: typeof take }> = sending,
): Promise<void> {
  return before.then(load).then(
    (chunk) => chunk.take(files, held, changed, kept, alive),
    async () => {
      const shell = snapshot("reviewShell") as { gone(): Promise<boolean> } | undefined;
      if (!alive() || (await shell?.gone())) return;
      held.trouble = UNSENT;
      changed();
    },
  );
}

/** A composer's attachments, under `key` (unique among the composers mounted at once). */
export function useAttach(key: string): Attach {
  const [held] = useState<Attachments>(() => ({
    files: restored(load(storageKey(key)) ?? ""),
    pending: [],
    trouble: null,
  }));
  const [, drawn] = useReducer((n: number) => n + 1, 0);
  const keep = useRef(() => save(storageKey(key), saved(held.files)));
  keep.current = () => save(storageKey(key), saved(held.files));
  useEffect(() => {
    let alive = true;
    let queue = Promise.resolve();
    const id = ++next;
    style();
    zones.set(key, {
      id,
      take(files) {
        queue = sendAfter(
          queue,
          files,
          held,
          () => alive && drawn(),
          () => keep.current(),
          () => alive,
        );
      },
    });
    if (zones.size === 1) listen(true);
    return () => {
      alive = false;
      // A newer composer that took the same key keeps it.
      if (zones.get(key)?.id === id) zones.delete(key);
      if (zones.size === 0) listen(false);
    };
  }, [key, held]);
  return {
    zone: key,
    files: held.files,
    pending: held.pending,
    trouble: held.trouble,
    busy: held.pending.length > 0,
    empty: held.files.length === 0,
    compose: (text) => compose(text, held.files),
    clear() {
      held.files = [];
      held.trouble = null;
      keep.current();
      drawn();
    },
    remove(url) {
      held.files = held.files.filter((file) => file.url !== url);
      keep.current();
      drawn();
    },
  };
}

/**
 * The picker, the chips of the files attached so far and what went wrong, drawn inside the
 * composer's `data-attach` form. While the composer sends, a paste or a drop is not taken: it
 * would miss the post.
 */
export function AttachControls({
  attach,
  disabled = false,
}: {
  attach: Attach;
  disabled?: boolean;
}) {
  const full = attach.files.length + attach.pending.length >= MAX_FILES;
  return (
    <div className="attach" data-attach-off={disabled}>
      <label
        className={disabled || full ? "attach-pick disabled" : "attach-pick"}
        title="Attach pictures or documents: pick them, paste a screenshot or drop files here"
      >
        <input
          className="dx-input"
          type="file"
          multiple
          accept={UPLOAD_ACCEPT}
          disabled={disabled || full}
          data-attach-input="true"
        />
        <span aria-hidden="true">📎</span>
        {" Attach"}
      </label>
      <span className="attach-hint">or paste, or drop files here</span>
      {(attach.files.length > 0 || attach.pending.length > 0) && (
        <ul className="attach-files" aria-label="Attached files">
          {attach.files.map((file) => (
            <li key={file.url} className="attach-file" data-url={file.url}>
              {file.picture ? (
                <img src={file.url} alt="" width="40" height="40" loading="lazy" decoding="async" />
              ) : (
                <span className="attach-icon" aria-hidden="true">
                  📄
                </span>
              )}
              <span className="attach-name">{file.name}</span>
              <Button
                type="button"
                aria-label={`Remove ${file.name}`}
                disabled={disabled}
                onClick={() => attach.remove(file.url)}
              >
                ×
              </Button>
            </li>
          ))}
          {attach.pending.map((name, at) => (
            <li
              // biome-ignore lint/suspicious/noArrayIndexKey: a name may be on its way twice, as its place tells them apart
              key={`pending-${at}-${name}`}
              className="attach-file pending"
              role="status"
            >
              <span className="attach-name">{`Uploading ${name}…`}</span>
            </li>
          ))}
        </ul>
      )}
      {attach.trouble !== null && (
        <p className="attach-trouble" role="alert">
          {attach.trouble}
        </p>
      )}
    </div>
  );
}
