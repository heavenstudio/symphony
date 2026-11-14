// A document section's editor, as the Dioxus client's docs_live.rs drew it: a card's verdict or
// message, or a comment after a section. The draft lives outside the page's markup, saved on this
// device on every input, so a read that redraws the section never loses it; a closed draft holds
// its section in place (`busy`, document-live.ts) only once the section is gone.
//
// A card's verdict goes under a tap, an id kept on this device before it is sent: a retry of a save
// no reply answered, whatever its words, or of the same answer after a reload, never writes a
// second verdict. The feed answers such a save: once the card's latest copy shows the owner's line
// it brought, the save landed (`Unanswered`, document.ts).

import { createContext, type ReactNode, useContext, useEffect, useRef, useState } from "react";
import { AttachControls, useAttach } from "./attach";
import { Button } from "./button";
import {
  type Ending,
  ending,
  ownerLines,
  release,
  releasedFrom,
  tap,
  Unanswered,
  type Words,
  words,
} from "./document";
import type { DocumentLive } from "./document-live";
import { useMount } from "./mount";
import { scoped } from "./project";
import { Failure, post, said } from "./request";
import { SelectField } from "./select";
import { field, STALE, useStore } from "./shared";
import { load, save } from "./storage";
import { Textarea } from "./textarea";
import type { Document } from "./wire";
import { CHECKING } from "./words";

/** The document page's live model, which its sections' editors read the card's latest copy from. */
export const PageContext = createContext<DocumentLive | null>(null);

/** Moves focus to the element the id `id` names, if the page has it. */
export function focus(id: string): void {
  const element = document.getElementById(id);
  if (element instanceof HTMLElement) element.focus();
}

/** The kinds a comment takes, each its own name. */
const KINDS: [string, string][] = ["comment", "question", "decision"].map((kind) => [kind, kind]);

/** The verdicts a card's editor offers beside its message, each with its button's words. */
const VERDICTS: [string, string][] = [
  ["approved", "✓ Approve"],
  ["rejected", "✗ Reject"],
  ["question", "? Question"],
];

/**
 * Why a save cannot go now, or none: a file still uploading, or no words where they are needed (a
 * comment's first line is its title; a message may be files alone; a verdict needs no note).
 */
export function refusal(
  action: string,
  text: string,
  uploading: boolean,
  files: boolean,
): string | null {
  if (uploading) return "A file is still uploading";
  if (text.trim() === "" && (action === "comment" || (action === "message" && !files))) {
    return "Type a message first";
  }
  return null;
}

/**
 * The post a save in a section sends, and its route: a card's message, a card's verdict under its
 * tap (`tapId`), or a comment after the section's last line.
 */
export function request(
  action: string,
  note: string,
  path: string,
  slug: string | null,
  tapId: string,
  afterLine: number,
  kind: string,
): [string, Record<string, unknown>] {
  const fields: Record<string, unknown> = { path, text: note };
  if (slug === null) return ["/api/comment", { ...fields, after_line: afterLine, kind }];
  if (action === "message") return ["/api/message", { ...fields, slug }];
  return ["/api/decision", { ...fields, slug, state: action, note, tap: tapId }];
}

/**
 * The words the server holds once it answered `reply`: those sent, else, for a retried tap it had
 * written already (`repeat`), the ones its reply names, which may be an earlier sending's.
 */
export function heldBy(reply: unknown, sent: Words): Words | null {
  if (field(reply, "repeat") !== true) return sent;
  const state = field(reply, "state");
  const note = field(reply, "note");
  return typeof state === "string" && typeof note === "string" ? words(state, note) : null;
}

export interface EditorProps {
  path: string;
  scope: string;
  sectionKey: string;
  /** The card's slug, for a card's editor; none for a comment's. */
  slug: string | null;
  /** The card's verdict now, which its buttons mark as current. */
  state: string;
  /** The section's last line, which a comment goes after. */
  afterLine: number;
  editing: boolean;
  setEditing: (editing: boolean) => void;
  /** The id of the button that opens this editor, where Escape returns focus. */
  opener: string;
  /** The section is gone from the latest copy: a save to its old place is refused. */
  gone: boolean;
  /** The card's Later, beside its verdicts. */
  later?: ReactNode;
}

export function Editor({
  path,
  scope,
  sectionKey,
  slug,
  state,
  afterLine,
  editing,
  setEditing,
  opener,
  gone,
  later = null,
}: EditorProps) {
  const page = useContext(PageContext);
  const mount = useMount();
  const key = `review-draft:${path}:${sectionKey}`;
  const tapKey = `review-tap:${path}:${sectionKey}`;
  const [text, setTextNow] = useState(() => load(key) ?? "");
  const typed = useRef(text);
  const setText = (value: string) => {
    typed.current = value;
    setTextNow(value);
  };
  const [kind, setKind] = useState("comment");
  const [pending, setPendingNow] = useState(false);
  const sending = useRef(false);
  const setPending = (value: boolean) => {
    sending.current = value;
    setPendingNow(value);
  };
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  // The card's save that no reply answered: the server may have written it.
  const [unanswered, setUnansweredNow] = useState<Unanswered | null>(null);
  const waiting = useRef<Unanswered | null>(null);
  const setUnanswered = (value: Unanswered | null) => {
    waiting.current = value;
    setUnansweredNow(value);
  };
  const attach = useAttach(key);
  const stale = useStore(STALE);
  const fieldId = `draft-${encodeURIComponent(key)}`;
  const message = slug !== null;

  useEffect(() => {
    if (editing) focus(fieldId);
  }, [editing, fieldId]);

  // The card's latest copy, followed only while a save waits for the feed to answer it.
  const [latest, setLatest] = useState<Document | null>(null);
  useEffect(() => {
    if (unanswered === null || page === null) return;
    const look = () => setLatest(page.state?.latest ?? null);
    look();
    return page.subscribe(look);
  }, [page, unanswered]);

  /**
   * Leaves the editor as `end` says once its save is answered: its status said, the box and the
   * draft emptied when the words are the server's, and the form closed when the owner wrote
   * nothing since sending. A form left open would hold the card in its place once its release
   * ends, and the answer's regroup with it.
   */
  const endSave = (end: Ending) => {
    if (end.clear) {
      setText("");
      attach.clear();
      save(key, "");
    }
    setSaved(end.status);
    if (end.close) setEditing(false);
  };

  // The feed answers a save no reply answered: once the card's latest copy shows the owner's line
  // it brought, the save landed. The card is released and the notice withdrawn; the line tells
  // which words under the tap the server holds, and only those are cleared from the box and said
  // to be saved (`ending`).
  // biome-ignore lint/correctness/useExhaustiveDependencies: it runs on what the feed brought; the draft and the files are read as they are then
  useEffect(() => {
    if (pending || unanswered === null || latest === null || page === null) return;
    if (!unanswered.answered(latest, sectionKey)) return;
    const last = unanswered.last();
    if (last === null) return;
    const held = unanswered.held(latest, sectionKey);
    setUnanswered(null);
    release(page.released, sectionKey, unanswered.before);
    save(tapKey, "");
    setError(null);
    endSave(ending(held, last, attach.compose(typed.current)));
  }, [pending, unanswered, latest, page, sectionKey, tapKey]);

  const send = async (action: string) => {
    if (gone) {
      setError("This block was removed; your draft is kept.");
      return;
    }
    // While the page may show what is out of date, the card may have moved since: wait for it.
    if (sending.current || STALE.get()) return;
    const refused = refusal(action, typed.current, attach.busy, !attach.empty);
    if (refused !== null) {
      setError(refused);
      return;
    }
    const note = attach.compose(typed.current);
    setPending(true);
    setError(null);
    setSaved(null);
    // The owner's own answer shows on its card at once, whatever else holds the card open: the
    // card's lines of the owner's before it are read as the save is sent, since the feed may bring
    // its change before the answer to the post.
    const before =
      slug === null ? null : page?.state ? ownerLines(page.state.latest, sectionKey) : 0;
    let tapId: string | null = null;
    if (slug !== null && action !== "message") {
      // Kept before it is sent: a retry of an unanswered tap, whatever its words, or of the same
      // answer after a reload, cannot append a second verdict.
      const signature = `${action}\n${note}`;
      tapId = tap(waiting.current, load(tapKey), signature) ?? `ui-${Date.now()}-${Math.random()}`;
      save(tapKey, `${tapId}\n${signature}`);
    }
    const [route, fields] = request(action, note, path, slug, tapId ?? "", afterLine, kind);
    const sent = words(action, note);
    try {
      const reply = await post(scoped(scope, route), fields);
      if (mount.aborted) return;
      const held = heldBy(reply, sent);
      // The card's save no reply answered, which this one retried under its tap.
      const earlier = waiting.current;
      setUnanswered(null);
      const retried = earlier?.retriedBy(tapId) === true ? earlier : null;
      const from =
        before === null ? null : releasedFrom(field(reply, "repeat") === true, before, retried);
      if (from !== null && page !== null) release(page.released, sectionKey, from);
      save(tapKey, "");
      endSave(ending(held, sent, attach.compose(typed.current)));
    } catch (failure) {
      if (mount.aborted) return;
      // No reply says whether it was written: the feed will, if it was.
      if (failure instanceof Failure && failure.kind === "uncertain" && before !== null) {
        setUnanswered(Unanswered.after(waiting.current, sent, tapId, before));
      }
      setError(said(failure));
    }
    setPending(false);
  };

  const submit = () => void send(message ? "message" : "comment");
  const heldLabel = (label: string) => (stale ? CHECKING : label);
  const drafted = text !== "" || !attach.empty;
  return (
    <div
      className="editor"
      data-busy={editing || pending || drafted || attach.busy ? "true" : "false"}
      // A closed draft holds its card's words, not the page's markup or order.
      data-draft={!editing && !pending && !attach.busy && drafted ? "true" : "false"}
    >
      {drafted && !editing && (
        <Button type="button" onClick={() => setEditing(true)}>
          Resume draft
        </Button>
      )}
      <form
        className={message ? "verdict" : "comment"}
        hidden={!editing}
        data-attach={attach.zone}
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        // Escape closes the editor, its draft kept, and returns focus to the button that opened
        // it. An open list in the form takes its own Escape and stops it there.
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            setEditing(false);
            focus(opener);
          }
        }}
      >
        <Textarea
          id={fieldId}
          value={text}
          disabled={pending}
          aria-label="Message or verdict note"
          placeholder="Write a message, or add a note to your verdict…"
          onChange={(event) => {
            save(key, event.target.value);
            setText(event.target.value);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              submit();
            }
          }}
        />
        <AttachControls attach={attach} disabled={pending} />
        {message && (
          // Approve is what the owner most often does, so it alone is filled; the card's own
          // verdict is marked as current, never promoted.
          <div className="verdict-choices">
            {VERDICTS.map(([value, label]) => (
              <Button
                key={value}
                type="button"
                className={value === "approved" ? "v-approved primary" : `v-${value}`}
                aria-current={value === state ? "true" : undefined}
                disabled={pending || stale}
                onClick={() => void send(value)}
              >
                {heldLabel(label)}
              </Button>
            ))}
            {later}
          </div>
        )}
        <div className="row">
          {!message && (
            <SelectField label="Comment kind" value={kind} onChange={setKind} options={KINDS} />
          )}
          <Button type="submit" className={message ? "" : "primary"} disabled={pending || stale}>
            {pending ? "Saving…" : stale ? CHECKING : message ? "Send message" : "Save comment"}
          </Button>
          {message && (
            <Button
              type="button"
              className="v-dismissed"
              aria-current={state === "dismissed" ? "true" : undefined}
              disabled={pending || stale}
              onClick={() => void send("dismissed")}
            >
              {heldLabel("⊘ Dismiss")}
            </Button>
          )}
          {/* A pending card has no verdict to take back. */}
          {message && state !== "pending" && (
            <Button
              type="button"
              variant="ghost"
              className="reset"
              title="Take back the verdict: the card waits for yours again"
              disabled={pending || stale}
              onClick={() => void send("pending")}
            >
              {heldLabel("↺ Reset to pending")}
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            className="discard"
            title="Clear what you wrote and close"
            disabled={pending}
            onClick={() => {
              setText("");
              attach.clear();
              save(key, "");
              setEditing(false);
            }}
          >
            Discard draft
          </Button>
        </div>
      </form>
      {error !== null && <p role="alert">{error}</p>}
      {saved !== null && <p role="status">{saved}</p>}
    </div>
  );
}
