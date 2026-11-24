// A document section's editor, as the Dioxus client's docs_live.rs drew it: a card's verdict or
// message, its verdicts drawn for whoever decides the card alone (`decides`), or a comment after a
// section. The draft lives outside the page's markup, saved on this
// device on every input, so a read that redraws the section never loses it; a closed draft holds
// its section in place (`busy`, document-live.ts) only once the section is gone.
//
// A card's verdict goes under a tap, an id kept on this device before it is sent: a retry of a save
// no reply answered, whatever its words, or of the same answer after a reload, never writes a
// second verdict. The feed answers such a save: once the card's latest copy shows the owner's line
// it brought, the save landed (`Unanswered`, document.ts).

import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import { type Attach, AttachControls, useAttach } from "./attach";
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

/** What a section's save reads of its editor, given again each time the editor is drawn. */
export interface SaveField {
  path: string;
  scope: string;
  sectionKey: string;
  /** The card's slug, for a card's editor; none for a comment's. */
  slug: string | null;
  afterLine: number;
  /** The comment's kind. */
  kind: string;
  /** The section is gone from the latest copy. */
  gone: boolean;
  /** The page's live model, whose latest copy and releases a card's save reads and sets. */
  page: DocumentLive | null;
  attach: Pick<Attach, "busy" | "empty" | "compose" | "clear">;
  setEditing: (editing: boolean) => void;
}

/** What a save shows in its editor, each one of the editor's states. */
export interface SaveShown {
  text: (value: string) => void;
  pending: (value: boolean) => void;
  error: (value: string | null) => void;
  saved: (value: string | null) => void;
  unanswered: (value: Unanswered | null) => void;
}

/**
 * A section's save, from its box's words to the server's answer, or the feed's when no reply
 * answered. It holds what the editor's handlers must read as it is now rather than as last drawn
 * (the words typed, whether a save is on its way, the save no reply answered), and nothing once
 * `mount` is aborted, the page gone while the server answered.
 */
export class SaveControl {
  /** The box's words as typed, ahead of the draw that shows them. */
  typed: string;
  /** A save is on its way: another waits for its answer. */
  sending = false;
  /** The card's save that no reply answered: the server may have written it. */
  waiting: Unanswered | null = null;

  constructor(
    typed: string,
    public field: SaveField,
    private readonly mount: AbortSignal,
    private readonly shown: SaveShown,
  ) {
    this.typed = typed;
  }

  /** Where this device keeps the section's draft. */
  get draftKey(): string {
    return `review-draft:${this.field.path}:${this.field.sectionKey}`;
  }

  /** Where this device keeps the tap a card's verdict goes under until a reply answers it. */
  get tapKey(): string {
    return `review-tap:${this.field.path}:${this.field.sectionKey}`;
  }

  private setText(value: string): void {
    this.typed = value;
    this.shown.text(value);
  }

  /** The owner typed `value` in the box: it is the draft, kept on this device at once. */
  type(value: string): void {
    save(this.draftKey, value);
    this.setText(value);
  }

  /** Discard draft: the box, its files and the draft kept emptied, and the form closed. */
  discard(): void {
    this.setText("");
    this.field.attach.clear();
    save(this.draftKey, "");
    this.field.setEditing(false);
  }

  private setPending(value: boolean): void {
    this.sending = value;
    this.shown.pending(value);
  }

  private setUnanswered(value: Unanswered | null): void {
    this.waiting = value;
    this.shown.unanswered(value);
  }

  /**
   * Leaves the editor as `end` says once its save is answered: its status said, the box and the
   * draft emptied when the words are the server's, and the form closed when the owner wrote
   * nothing since sending. A form left open would hold the card in its place once its release
   * ends, and the answer's regroup with it.
   */
  private end(end: Ending): void {
    if (end.clear) {
      this.setText("");
      this.field.attach.clear();
      save(this.draftKey, "");
    }
    this.shown.saved(end.status);
    if (end.close) this.field.setEditing(false);
  }

  /**
   * The card's latest copy the feed brought, while a save no reply answered waits: once it shows
   * the owner's line that save brought, the save landed. The card is released and the notice
   * withdrawn; the line tells which words under the tap the server holds, and only those are
   * cleared from the box and said to be saved (`ending`).
   */
  heard(latest: Document): void {
    const { page, sectionKey } = this.field;
    const unanswered = this.waiting;
    if (this.sending || unanswered === null || page === null) return;
    if (!unanswered.answered(latest, sectionKey)) return;
    const last = unanswered.last();
    if (last === null) return;
    const held = unanswered.held(latest, sectionKey);
    this.setUnanswered(null);
    release(page.released, sectionKey, unanswered.before);
    save(this.tapKey, "");
    this.shown.error(null);
    this.end(ending(held, last, this.field.attach.compose(this.typed)));
  }

  /** Save the box's words as `action`: a comment, a card's message, or a card's verdict. */
  async send(action: string): Promise<void> {
    const { gone, slug, page, sectionKey, attach, path, scope, afterLine, kind } = this.field;
    if (gone) {
      this.shown.error("This block was removed; your draft is kept.");
      return;
    }
    // While the page may show what is out of date, the card may have moved since: wait for it.
    if (this.sending || STALE.get()) return;
    const refused = refusal(action, this.typed, attach.busy, !attach.empty);
    if (refused !== null) {
      this.shown.error(refused);
      return;
    }
    const note = attach.compose(this.typed);
    this.setPending(true);
    this.shown.error(null);
    this.shown.saved(null);
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
      tapId =
        tap(this.waiting, load(this.tapKey), signature) ?? `ui-${Date.now()}-${Math.random()}`;
      save(this.tapKey, `${tapId}\n${signature}`);
    }
    const [route, fields] = request(action, note, path, slug, tapId ?? "", afterLine, kind);
    const sent = words(action, note);
    try {
      const reply = await post(scoped(scope, route), fields);
      if (this.mount.aborted) return;
      const held = heldBy(reply, sent);
      // The card's save no reply answered, which this one retried under its tap.
      const earlier = this.waiting;
      this.setUnanswered(null);
      const retried = earlier?.retriedBy(tapId) === true ? earlier : null;
      const from =
        before === null ? null : releasedFrom(field(reply, "repeat") === true, before, retried);
      if (from !== null && page !== null) release(page.released, sectionKey, from);
      save(this.tapKey, "");
      this.end(ending(held, sent, attach.compose(this.typed)));
    } catch (failure) {
      if (this.mount.aborted) return;
      // No reply says whether it was written: the feed will, if it was.
      if (failure instanceof Failure && failure.kind === "uncertain" && before !== null) {
        this.setUnanswered(Unanswered.after(this.waiting, sent, tapId, before));
      }
      this.shown.error(said(failure));
    }
    this.setPending(false);
  }
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
  /** The card's Later: beside its verdicts, alone where none is drawn. */
  later?: ReactNode;
  /** Whether the one reading decides the card (owner.tsx's `decides`): its verdicts, Dismiss
   * and Reset are drawn for them alone; its message and Later, a move in the queue, for
   * everyone. */
  decides?: boolean;
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
  decides = true,
}: EditorProps) {
  const page = useContext(PageContext);
  const mount = useMount();
  const key = `review-draft:${path}:${sectionKey}`;
  const [text, setText] = useState(() => load(key) ?? "");
  const [kind, setKind] = useState("comment");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  // The card's save that no reply answered: the server may have written it.
  const [unanswered, setUnanswered] = useState<Unanswered | null>(null);
  const attach = useAttach(key);
  const drawn = { path, scope, sectionKey, slug, afterLine, kind, gone, page, attach, setEditing };
  const [control] = useState(
    () =>
      new SaveControl(text, drawn, mount, {
        text: setText,
        pending: setPending,
        error: setError,
        saved: setSaved,
        unanswered: setUnanswered,
      }),
  );
  control.field = drawn;
  const stale = useStore(STALE);
  const fieldId = `draft-${encodeURIComponent(key)}`;
  const message = slug !== null;
  // A card's verdicts, for whoever decides it.
  const verdicts = message && decides;

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

  // The feed answers a save no reply answered (`SaveControl.heard`), once that save's own post
  // is over.
  useEffect(() => {
    if (pending || unanswered === null || latest === null) return;
    control.heard(latest);
  }, [pending, unanswered, latest, control]);

  const send = (action: string) => control.send(action);
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
          aria-label={message && !decides ? "Message" : "Message or verdict note"}
          placeholder={
            message && !decides
              ? "Write a message…"
              : "Write a message, or add a note to your verdict…"
          }
          onChange={(event) => control.type(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              submit();
            }
          }}
        />
        <AttachControls attach={attach} disabled={pending} />
        {verdicts && (
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
        {!verdicts && later}
        <div className="row">
          {!message && (
            <SelectField label="Comment kind" value={kind} onChange={setKind} options={KINDS} />
          )}
          <Button type="submit" className={message ? "" : "primary"} disabled={pending || stale}>
            {pending ? "Saving…" : stale ? CHECKING : message ? "Send message" : "Save comment"}
          </Button>
          {verdicts && (
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
          {verdicts && state !== "pending" && (
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
            onClick={() => control.discard()}
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
