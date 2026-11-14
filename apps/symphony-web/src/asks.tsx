// A document's questions, each tied to a place in it, as the Dioxus client's asks.rs drew them.
// Drafts stay on this device; only a send posts a question or a follow-up. A read replaces the
// threads, never a half-typed answer: each draft is its composer's own.

import { useEffect, useState } from "react";
import { AttachControls, useAttach } from "./attach";
import { BlocksView } from "./blocks";
import { Button } from "./button";
import { readAsks } from "./document-read";
import { sleep } from "./feed";
import { useMount } from "./mount";
import { getJson, post, said } from "./request";
import { field, list, STALE, useStore } from "./shared";
import { load, save } from "./storage";
import { Textarea } from "./textarea";
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
    const signal = reading.signal;
    void (async () => {
      while (!signal.aborted) {
        try {
          const read = readAsks(await getJson(endpoint, signal));
          if (signal.aborted) return;
          if (read === null) throw new Error("The server's asks could not be read");
          setAsks((now) => (JSON.stringify(now) === JSON.stringify(read.asks) ? now : read.asks));
          setRecipient(read.recipient);
          setTrouble(null);
          return;
        } catch (error) {
          if (signal.aborted) return;
          setTrouble(said(error));
        }
        await sleep(3_000, signal);
      }
    })();
    return () => reading.abort();
  }, [endpoint, heard]);
  // A question or a follow-up sent is read at once, without waiting for the feed.
  useEffect(() => {
    if (sent === 0) return;
    const reading = new AbortController();
    void getJson(endpoint, reading.signal).then(
      (value) => {
        const read = readAsks(value);
        if (read !== null && !reading.signal.aborted) setAsks(read.asks);
      },
      () => {},
    );
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

/** Whether a composer may send: nothing on its way, and words or a file to send. */
const ready = (text: string, empty: boolean) => text.trim() !== "" || !empty;

function NewAsk({ path, scope, onSent }: AsksProps & { onSent: () => void }) {
  const key = `review-ask:${scope}/${path}`;
  const [text, setText] = useState(() => load(key) ?? "");
  const [anchor, setAnchor] = useState("");
  const [quote, setQuote] = useState("");
  const [sending, setSending] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const attach = useAttach(key);
  const mount = useMount();
  // The question waits while the page may show what is out of date; the draft stays editable.
  const stale = useStore(STALE);
  const send = async () => {
    if (sending || STALE.get() || attach.busy || !ready(text, attach.empty)) return;
    setSending(true);
    setTrouble(null);
    const question = attach.compose(text);
    try {
      await post(`${scope}/api/ask`, { path, question, anchor, quote });
      if (mount.aborted) return;
      setText("");
      attach.clear();
      save(key, "");
      onSent();
    } catch (error) {
      if (mount.aborted) return;
      setTrouble(said(error));
    }
    setSending(false);
  };
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
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
        data-attach={attach.zone}
      >
        <label htmlFor="new-ask">Question</label>
        <Textarea
          id="new-ask"
          value={text}
          maxLength={4000}
          required={attach.empty}
          disabled={sending}
          onChange={(event) => {
            setText(event.target.value);
            save(key, event.target.value);
          }}
        />
        <AttachControls attach={attach} disabled={sending} />
        {trouble !== null && <p role="alert">{trouble}</p>}
        <Button
          type="submit"
          disabled={sending || stale || attach.busy || !ready(text, attach.empty)}
        >
          {sending ? "Sending…" : stale ? CHECKING : "Ask"}
        </Button>
      </form>
    </>
  );
}

export function AskThread({ ask, scope, onSent }: { ask: Ask; scope: string; onSent: () => void }) {
  const [shown, watch] = useVisibility();
  const key = `review-ask-reply:${ask.id}`;
  const [text, setText] = useState(() => load(key) ?? "");
  const [sending, setSending] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const attach = useAttach(key);
  const mount = useMount();
  // The follow-up waits while the page may show what is out of date; the draft stays editable.
  const stale = useStore(STALE);
  const send = async () => {
    if (sending || STALE.get() || attach.busy || !ready(text, attach.empty)) return;
    setSending(true);
    setTrouble(null);
    const message = attach.compose(text);
    try {
      await post(`${scope}/api/ask-reply`, { id: String(ask.id), text: message });
      if (mount.aborted) return;
      setText("");
      attach.clear();
      save(key, "");
      onSent();
    } catch (error) {
      if (mount.aborted) return;
      setTrouble(said(error));
    }
    setSending(false);
  };
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
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void send();
            }}
            data-attach={attach.zone}
          >
            <label htmlFor={`reply-${ask.id}`}>Follow up</label>
            <Textarea
              id={`reply-${ask.id}`}
              value={text}
              maxLength={4000}
              required={attach.empty}
              disabled={sending}
              onChange={(event) => {
                setText(event.target.value);
                save(key, event.target.value);
              }}
            />
            <AttachControls attach={attach} disabled={sending} />
            {trouble !== null && <p role="alert">{trouble}</p>}
            <Button
              type="submit"
              disabled={sending || stale || attach.busy || !ready(text, attach.empty)}
            >
              {sending ? "Sending…" : stale ? CHECKING : "Send follow-up"}
            </Button>
          </form>
        </>
      )}
    </article>
  );
}
