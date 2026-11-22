// A document's questions, each tied to a place in it, as the Dioxus client's asks.rs drew them.
// Drafts stay on this device; only a send posts a question or a follow-up. A read replaces the
// threads, never a half-typed answer: each draft is its composer's own.

import { useEffect, useState } from "react";
import { type Attach, AttachControls, useAttach } from "./attach";
import { BlocksView } from "./blocks";
import { Button } from "./button";
import { readAsks } from "./document-read";
import { useMount } from "./mount";
import { getJson, post, said } from "./request";
import { field, list, STALE, useStore } from "./shared";
import { load, save } from "./storage";
import { Textarea } from "./textarea";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: a failed read of the asks is tried again on a fixed RETRY_MS, where Backoff and retryWait are the helpers
import { sleep } from "./timing";
import { useVisibility } from "./viewport";
import type { Ask } from "./wire";
import { CHECKING } from "./words";

/**
 * Whether what the page's feed heard of the asks (`moved`, as the frame says it on `review:asks`)
 * concerns the threads of the document `path`: the stream opened again (`true`), the chat's
 * recipient changed, or one of its threads has a new message.
 */
export function concerns(moved: unknown, path: string): boolean {
  if (moved === true) return true;
  if (typeof moved !== "object" || moved === null) return false;
  if (Object.hasOwn(moved, "recipient")) return true;
  return list(field(moved, "threads")).some((thread) => field(thread, "path") === path);
}

/**
 * The quote the reader selected and the last heading of the document before it, or the address's
 * fragment for a question about a whole section. A selection outside the document, a form's
 * included, is never attached.
 */
export function selection(): [string, string] {
  let anchor = location.hash.replace(/^#/, "");
  const picked = getSelection();
  if (!picked) return [anchor, ""];
  const quote = picked.toString();
  const node = picked.anchorNode;
  if (!node) return [anchor, ""];
  const element = node instanceof Element ? node : node.parentElement;
  if (!element) return [anchor, ""];
  if (element.closest(".asks, input, textarea")) return ["", ""];
  const main = element.closest("main");
  if (!main) return ["", ""];
  for (const heading of main.querySelectorAll("h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]")) {
    if (
      heading.contains(node) ||
      (heading.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0
    ) {
      anchor = heading.id;
    }
  }
  return [anchor, [...quote].slice(0, 4000).join("")];
}

/** What a read of a document's asks shows: its threads, the chat's recipient, and why it failed. */
export interface AsksShown {
  asks: (asks: Ask[]) => void;
  recipient: (recipient: string | null) => void;
  trouble: (trouble: string | null) => void;
}

/** How long a failed read of the asks waits before it is asked again. */
export const RETRY_MS = 3_000;

/**
 * Read the asks at `endpoint` until a read answers, asking again `RETRY_MS` after each that
 * failed and saying why it did; nothing more once `signal` ended, its request given up with it.
 */
export async function readAsksUntilAnswered(
  endpoint: string,
  signal: AbortSignal,
  shown: AsksShown,
): Promise<void> {
  while (!signal.aborted) {
    try {
      const read = readAsks(await getJson(endpoint, signal));
      if (signal.aborted) return;
      if (read === null) throw new Error("The server's asks could not be read");
      shown.asks(read.asks);
      shown.recipient(read.recipient);
      shown.trouble(null);
      return;
    } catch (error) {
      if (signal.aborted) return;
      shown.trouble(said(error));
    }
    await sleep(RETRY_MS, signal);
  }
}

/**
 * Read the asks at `endpoint` once, after a question or a follow-up was sent, without waiting for
 * the feed: its threads shown when it answers before `signal` ends, nothing said when it fails
 * (the feed's next read says it).
 */
export async function readAsksAgain(
  endpoint: string,
  signal: AbortSignal,
  shown: (asks: Ask[]) => void,
): Promise<void> {
  try {
    const read = readAsks(await getJson(endpoint, signal));
    if (read !== null && !signal.aborted) shown(read.asks);
  } catch {
    // The feed's next read says what failed.
  }
}

/** Whether a composer may send: words or a file to send. */
const ready = (text: string, empty: boolean) => text.trim() !== "" || !empty;

/** What a composer's send reads of its composer as drawn, and what it sets there. */
export interface Draft {
  /** Where this device keeps the draft. */
  key: string;
  text: string;
  sending: boolean;
  attach: Pick<Attach, "busy" | "empty" | "compose" | "clear">;
  /** The composer's mount: nothing is set once it ended. */
  mount: AbortSignal;
  setText: (text: string) => void;
  setSending: (sending: boolean) => void;
  setTrouble: (trouble: string | null) => void;
  /** What a send that went through does next: the asks read again. */
  onSent: () => void;
}

/**
 * Post `draft`'s words with its files' lines to `route`, the fields `fields` makes of them: once
 * sent, the box, its files and the draft kept are emptied (`onSent` after); a failure says why,
 * the draft kept. Nothing is sent while a send is on its way, the page may show what is out of
 * date, a file is still uploading or there is nothing to send, and nothing more is set once the
 * composer's mount ended while the server answered.
 */
export async function sendDraft(
  draft: Draft,
  route: string,
  fields: (composed: string) => Record<string, unknown>,
): Promise<void> {
  const { attach } = draft;
  if (draft.sending || STALE.get() || attach.busy || !ready(draft.text, attach.empty)) return;
  draft.setSending(true);
  draft.setTrouble(null);
  try {
    await post(route, fields(attach.compose(draft.text)));
    if (draft.mount.aborted) return;
    draft.setText("");
    attach.clear();
    save(draft.key, "");
    draft.onSent();
  } catch (error) {
    if (draft.mount.aborted) return;
    draft.setTrouble(said(error));
  }
  draft.setSending(false);
}

export interface AsksProps {
  /** The document's path, as its page's route names it. */
  path: string;
  /** Where its asks are read and sent: `""`, or a checkout's `/@SCOPE`. */
  scope: string;
}

/**
 * The document's questions, read once the panel comes near the screen. Keyed by the scope and the
 * path when the page moves to another document, as the document's feed is.
 */
export function Asks({ path, scope }: AsksProps) {
  const [shown, watch] = useVisibility();
  return (
    <section className="asks-shell" ref={watch}>
      {shown ? <AsksBody path={path} scope={scope} /> : <h2>Questions about this document</h2>}
    </section>
  );
}

function AsksBody({ path, scope }: AsksProps) {
  const [asks, setAsks] = useState<Ask[]>([]);
  const [recipient, setRecipient] = useState<string | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);
  const [heard, setHeard] = useState(0);
  const [sent, setSent] = useState(0);
  const endpoint = `${scope}/api/asks?format=blocks&path=${encodeURIComponent(path)}`;
  // The page's feed says when a thread of this document moves, the chat's recipient changes or the
  // stream opens again; the questions are read then, never on a timer, and a read that news
  // replaces is given up, its request with it. A failed read tries again until one answers.
  useEffect(() => {
    const moved = (event: Event) => {
      if (concerns((event as Event & { moved?: unknown }).moved, path)) {
        setHeard((count) => count + 1);
      }
    };
    addEventListener("review:asks", moved);
    return () => removeEventListener("review:asks", moved);
  }, [path]);
  useEffect(() => {
    void heard;
    const reading = new AbortController();
    void readAsksUntilAnswered(endpoint, reading.signal, {
      asks: (read) => setAsks((now) => (JSON.stringify(now) === JSON.stringify(read) ? now : read)),
      recipient: setRecipient,
      trouble: setTrouble,
    });
    return () => reading.abort();
  }, [endpoint, heard]);
  // A question or a follow-up sent is read at once, without waiting for the feed.
  useEffect(() => {
    if (sent === 0) return;
    const reading = new AbortController();
    void readAsksAgain(endpoint, reading.signal, setAsks);
    return () => reading.abort();
  }, [endpoint, sent]);
  const onSent = () => setSent((count) => count + 1);
  return (
    <section className="asks" id="asks" aria-label="Questions about this document">
      <h2>Ask about this document</h2>
      <p className="muted">
        {recipient !== null
          ? `Chat goes to ${recipient}.`
          : "Nobody is registered to receive chat. Your question is queued."}
        {" Questions go after a day without activity."}
      </p>
      {trouble !== null && <p role="status">{`Questions could not be refreshed: ${trouble}`}</p>}
      <NewAsk path={path} scope={scope} onSent={onSent} />
      {asks.map((ask) => (
        <AskThread key={ask.id} ask={ask} scope={scope} onSent={onSent} />
      ))}
    </section>
  );
}

/** A question's or a follow-up's draft as its form draws it: what `sendDraft` sends, with what a
 * failed send said and whether the page may show what is out of date. */
export interface AskDraft extends Draft {
  attach: Attach;
  trouble: string | null;
  stale: boolean;
}

/**
 * The draft kept under `key` in this browser, with its attachments, which tells `onSent` once it
 * was sent. It waits while the page may show what is out of date, and stays editable.
 */
export function useAskDraft(key: string, onSent: () => void): AskDraft {
  const [text, setText] = useState(() => load(key) ?? "");
  const [sending, setSending] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const attach = useAttach(key);
  const mount = useMount();
  const stale = useStore(STALE);
  return {
    key,
    text,
    sending,
    attach,
    mount,
    setText,
    setSending,
    setTrouble,
    onSent,
    trouble,
    stale,
  };
}

/** A draft's form: its box under `label`, its attachments, what a failed send said and its submit,
 * which says `word` and calls `onSend`. */
export function AskForm({
  draft,
  id,
  label,
  word,
  onSend,
}: {
  draft: AskDraft;
  id: string;
  label: string;
  word: string;
  onSend: () => Promise<void>;
}) {
  const { key, text, sending, attach, trouble, stale } = draft;
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        void onSend();
      }}
      data-attach={attach.zone}
    >
      <label htmlFor={id}>{label}</label>
      <Textarea
        id={id}
        value={text}
        maxLength={4000}
        required={attach.empty}
        disabled={sending}
        onChange={(event) => {
          draft.setText(event.target.value);
          save(key, event.target.value);
        }}
      />
      <AttachControls attach={attach} disabled={sending} />
      {trouble !== null && <p role="alert">{trouble}</p>}
      <Button
        type="submit"
        disabled={sending || stale || attach.busy || !ready(text, attach.empty)}
      >
        {sending ? "Sending…" : stale ? CHECKING : word}
      </Button>
    </form>
  );
}

function NewAsk({ path, scope, onSent }: AsksProps & { onSent: () => void }) {
  const key = `review-ask:${scope}/${path}`;
  const [anchor, setAnchor] = useState("");
  const [quote, setQuote] = useState("");
  const draft = useAskDraft(key, onSent);
  const send = () =>
    sendDraft(draft, `${scope}/api/ask`, (question) => ({ path, question, anchor, quote }));
  return (
    <>
      <Button
        type="button"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          const [picked, words] = selection();
          setAnchor(picked);
          setQuote(words);
        }}
      >
        Use selected passage
      </Button>
      {(quote !== "" || anchor !== "") && (
        <>
          <blockquote>
            {anchor !== "" && <small>{`#${anchor}`}</small>}
            {quote}
          </blockquote>
          <Button
            type="button"
            onClick={() => {
              setAnchor("");
              setQuote("");
            }}
          >
            Clear passage
          </Button>
        </>
      )}
      <AskForm draft={draft} id="new-ask" label="Question" word="Ask" onSend={send} />
    </>
  );
}

export function AskThread({ ask, scope, onSent }: { ask: Ask; scope: string; onSent: () => void }) {
  const [shown, watch] = useVisibility();
  const key = `review-ask-reply:${ask.id}`;
  const draft = useAskDraft(key, onSent);
  const send = () =>
    sendDraft(draft, `${scope}/api/ask-reply`, (message) => ({
      id: String(ask.id),
      text: message,
    }));
  return (
    <article className="ask-thread card" id={`ask-${ask.id}`} data-deferred={!shown} ref={watch}>
      {shown && (
        <>
          <p className="muted">{`Question ${ask.id} · ${ask.status}`}</p>
          {ask.anchor !== "" && <a href={`#${ask.anchor}`}>{`#${ask.anchor}`}</a>}
          {ask.quote !== "" && <blockquote>{ask.quote}</blockquote>}
          {ask.messages.map((message) => (
            <div key={message.seq} className="ask-message">
              <p className="by">{message.by}</p>
              <BlocksView blocks={message.blocks ?? []} />
            </div>
          ))}
          <AskForm
            draft={draft}
            id={`reply-${ask.id}`}
            label="Follow up"
            word="Send follow-up"
            onSend={send}
          />
        </>
      )}
    </article>
  );
}
