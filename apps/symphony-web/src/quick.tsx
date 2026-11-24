// The `+` sheet (`#quick-sheet`): a chat about the page, or a task added, on two tabs. Its draft
// is shared with the frame (`reviewQuickDraft`, `review-quick-draft`, `review:quick`), so the frame
// draws the sheet's button and the page's room for it as the draft says, and a reload keeps what
// was typed. A task's save keeps its draft, and a request identity the server knows it by, until
// the server confirms it: a save whose outcome is unknown is checked again before anything else is
// sent. The open chat reads its thread when the page's feed says it moved, never on a timer.

import {
  type CSSProperties,
  type FormEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { AttachControls, useAttach } from "./attach";
import { BlocksView } from "./blocks";
import { Button } from "./button";
import { Input } from "./input";
import { useMount } from "./mount";
import { holdFocus, releaseFocus, returnFocus, takeFocus, useEscape } from "./panel-focus";
import {
  type AskReply,
  concernsThread,
  type Draft,
  NEW_DRAFT,
  readAskReply,
  readDraft,
  sameDraft,
  saveOutcome,
  titleLength,
  trimmed,
} from "./panel-model";
import { href, project, readmeRoute, scoped, strip, writer } from "./project";
import { Failure, getJson, post, said } from "./request";
import type { Route } from "./route";
import { SelectField } from "./select";
import { field, publish, remember, snapshot } from "./shared";
import { load, save } from "./storage";
import { TabContent, TabList, Tabs, TabTrigger } from "./tabs";
import { Textarea } from "./textarea";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: readThread tries again on a fixed RETRY_MS (Backoff is the helper), and saveTask asks the server again each CHECK_MS while it checks a save, where the feed's event of it is the wait
import { sleep } from "./timing";
import { Tooltip } from "./tooltip";
import { Skeleton } from "./viewport";
import { QUICK_TASK_TITLE_MAX } from "./wire";

/** The shown tab's panel: a stop for Tab in the sheet that a send or a save leaves enabled, where
 * it holds the focus while the fields, the tabs and Send are disabled. */
const SHOWN_PANEL = "#quick-sheet [role=tabpanel]:not([hidden])";

/** Where the focus goes when the sheet opens, the first of these that shows and can take it: the
 * field the reader types in, else the shown tab's panel while a save holds the fields disabled. */
const QUICK_FIRST = [`${SHOWN_PANEL} :is(#quick-title, #quick-text):not(:disabled)`, SHOWN_PANEL];

/** How long a conversation's failed read waits before it is tried again, in ms. */
export const RETRY_MS = 3_000;
/** How long a task save still being checked waits before it is asked again, in ms. */
export const CHECK_MS = 1_000;

const TABS = ["chat", "task"];
const PRIORITIES: [string, string][] = [0, 1, 2, 3, 4].map((priority) => [
  String(priority),
  `p${priority}`,
]);

/** The draft the window holds, else the one this device kept, else a new one. */
export function loadDraft(): Draft {
  const shared = readDraft(snapshot("reviewQuickDraft"));
  if (shared !== null) return shared;
  let kept: unknown;
  try {
    kept = JSON.parse(load("review-quick-draft") ?? "null");
  } catch {
    kept = null;
  }
  return readDraft(kept) ?? NEW_DRAFT;
}

/** `draft` left on the window for the frame and kept on this device. */
export function saveDraft(draft: Draft): void {
  remember("reviewQuickDraft", { ...draft });
  save("review-quick-draft", JSON.stringify(draft));
}

/** A change to the draft: the fields it sets, from the draft as it stands. */
export type ChangeDraft = (change: (draft: Draft) => Partial<Draft>) => void;

/**
 * The draft, and its change: a change that sets anything is kept (`saveDraft`) and said
 * (`review:quick`), and what another module says of it is taken. Mounted, the draft is kept and
 * said once, after the task that mounted it, so the frame draws what the panel loaded.
 */
export function useDraft(): [Draft, ChangeDraft] {
  const [draft, setDraft] = useState(loadDraft);
  const latest = useRef(draft);
  const keep = useCallback((next: Draft) => {
    latest.current = next;
    setDraft(next);
    saveDraft(next);
    publish("review:quick");
  }, []);
  const change = useCallback<ChangeDraft>(
    (changed) => {
      const next = { ...latest.current, ...changed(latest.current) };
      if (!sameDraft(next, latest.current)) keep(next);
    },
    [keep],
  );
  useEffect(() => {
    const heard = () => {
      const next = loadDraft();
      if (!sameDraft(next, latest.current)) keep(next);
    };
    addEventListener("review:quick", heard);
    // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: the draft is kept a turn after the mount, where the mount's own effect is the event
    const mounted = setTimeout(() => keep(latest.current), 0);
    return () => {
      removeEventListener("review:quick", heard);
      clearTimeout(mounted);
    };
  }, [keep]);
  return [draft, change];
}

/** A task save's request identity: 16 random bytes, in hex. */
export function requestIdentity(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** A draft's priority as the save sends it: the number it is, else p2. */
export function priorityOf(priority: string): number {
  return /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(priority) ? Number(priority) : 2;
}

/** An address's fragment (`hash`) without its `#`, decoded; empty for one that does not decode. */
export function fragmentOf(hash: string): string {
  try {
    return decodeURIComponent(hash.replace(/^#+/, ""));
  } catch {
    return "";
  }
}

/** Where the reader is: the address's path, fragment and whole, as `location` has them. */
export interface At {
  pathname: string;
  hash: string;
  href: string;
}

/** Whether the form has something to send and nothing holds it: a title for a task, a message or a
 * file for the chat, no send out and no file still being read. */
export function canSend(
  draft: Draft,
  sending: boolean,
  attach: { busy: boolean; empty: boolean },
): boolean {
  if (sending || attach.busy) return false;
  return draft.task ? trimmed(draft.title) !== "" : trimmed(draft.text) !== "" || !attach.empty;
}

/** The page a task's save names: the one its first send named (a reload sends the same request),
 * else a document's own address and anchor, else the page's path in its project and anchor. */
export function taskPage(sent: Draft, route: Route, at: At): string {
  if (sent.request_page !== "") return sent.request_page;
  return route.document && !route.folder
    ? scoped(route.scope, `/${route.path}${at.hash}`)
    : `${strip(project(), at.pathname)}${at.hash}`;
}

/** A new conversation's post (`/api/ask`), before its files are added to the question: about a
 * document by its path, else about the project's readme with the page the chat was sent from. */
export function askOf(route: Route, text: string, at: At) {
  const document = route.document && !route.folder;
  return {
    path: document ? route.path : readmeRoute(project()),
    question: document ? text : `${text}\n\n(sent from ${at.href})`,
    anchor: fragmentOf(at.hash),
  };
}

/** What a send that went through changes in the draft as it stands now (`now`), from the server's
 * answer (`value`) on the page of `scope`: a task's fields emptied; a reply's text; a new
 * conversation's text, its id and scope taken up. */
export function afterSent(now: Draft, value: unknown, scope: string): Partial<Draft> {
  if (now.task) {
    return { text: "", title: "", slug: "", request: "", request_page: "", request_scope: "" };
  }
  if (now.ask !== 0) return { text: "" };
  const id = field(value, "id");
  return { text: "", ask: typeof id === "number" ? Math.trunc(id) : 0, ask_scope: scope };
}

/**
 * The conversation at `url` (`/api/ask?format=blocks&id=`), read until one read answers, each
 * failure said to `shown.trouble` and tried again `RETRY_MS` later; the read drawn and the trouble
 * cleared once it does. A missing conversation is given up through `shown.missing`, with no
 * retry. Nothing is shown once `signal` ended.
 */
export async function readThread(
  url: string,
  signal: AbortSignal,
  shown: {
    thread: (read: AskReply) => void;
    trouble: (why: string | null) => void;
    missing: () => void;
  },
): Promise<void> {
  while (!signal.aborted) {
    try {
      const read = readAskReply(await getJson(url, signal));
      if (signal.aborted) return;
      if (read === null) throw new Error("The server's conversation could not be read");
      shown.thread(read);
      shown.trouble(null);
      return;
    } catch (failure) {
      if (signal.aborted) return;
      if (failure instanceof Failure && failure.kind === "missing") {
        shown.trouble(null);
        shown.missing();
        return;
      }
      shown.trouble(said(failure));
    }
    await sleep(RETRY_MS, signal);
  }
}

/**
 * A task's save (`/api/quick-task` at `url`), asked again each `CHECK_MS` while the server is still
 * checking it or its answer was uncertain, each reason said to `checking`: the slug it was saved
 * under, or none once `signal` ended. Any other failure is thrown.
 */
export async function saveTask(
  url: string,
  fields: Record<string, unknown>,
  signal: AbortSignal,
  checking: (why: string) => void,
): Promise<string | null> {
  while (!signal.aborted) {
    try {
      const outcome = saveOutcome(await post(url, fields));
      if ("saved" in outcome) return outcome.saved;
      if (signal.aborted) break;
      checking(outcome.checking);
    } catch (failure) {
      if (!(failure instanceof Failure) || failure.kind !== "uncertain") throw failure;
      if (signal.aborted) break;
      checking(failure.text);
    }
    await sleep(CHECK_MS, signal);
  }
  return null;
}

export interface QuickThreadProps {
  draft: Draft;
  /** The conversation the draft names, once read; none before. */
  shown: AskReply | null;
  /** Why the conversation's last read failed, or none. */
  readError: string | null;
  /** Why a task's save is still being checked, or none. */
  checking: string | null;
  /** The slug the last task was saved under, or none. */
  created: string | null;
}

/**
 * The sheet's thread: on the chat's tab the conversation, its ghost while it is read, or the
 * invitation to ask; on the Add task tab what the last save came to, and with nothing to say it is
 * quiet and takes no room, so the form sits under the tabs.
 */
export function QuickThread({ draft, shown, readError, checking, created }: QuickThreadProps) {
  const quiet = draft.task && checking === null && draft.request === "" && created === null;
  return (
    <div className={quiet ? "quick-thread quiet" : "quick-thread"} aria-live="polite">
      {!draft.task && readError !== null && (
        <p role="status">{`Conversation could not be refreshed: ${readError}`}</p>
      )}
      {!draft.task ? (
        draft.ask > 0 ? (
          shown !== null ? (
            <>
              {shown.ask.messages.map((message) => (
                <article className="quick-message" key={message.seq}>
                  <p className="by" title={writer(message.by).title}>
                    {writer(message.by).text}
                  </p>
                  <BlocksView blocks={message.blocks ?? []} />
                </article>
              ))}
              {(shown.recipient !== null || shown.ask.status !== "answered") && (
                <p className="muted">
                  {shown.recipient !== null
                    ? `Chat goes to ${shown.recipient}.`
                    : "Nobody is registered to receive chat; your message is queued."}
                </p>
              )}
            </>
          ) : (
            <>
              <span className="status-text" role="status">
                Loading conversation…
              </span>
              {[0, 1].map((ghost) => (
                <div key={ghost} className="ghost-message">
                  <div className="by">
                    <span className="bone" style={{ "--w": "9ch" } as CSSProperties} />
                  </div>
                  <Skeleton lines={2} />
                </div>
              ))}
            </>
          )
        ) : (
          <p className="muted">Ask about this page.</p>
        )
      ) : checking !== null ? (
        <p role="status">{`Checking save… ${checking}`}</p>
      ) : draft.request !== "" ? (
        <p role="status">Check the previous save to continue; your text is kept.</p>
      ) : created !== null ? (
        <p role="status">
          {"✓ Task "}
          <a href={href(`/task/${created}`)}>{created}</a>
          {" created"}
        </p>
      ) : null}
    </div>
  );
}

export interface QuickPanelProps {
  route: Route;
  draft: Draft;
  change: ChangeDraft;
}

export function QuickPanel({ route, draft, change }: QuickPanelProps) {
  const mount = useMount();
  const [thread, setThread] = useState<AskReply | null>(null);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [created, setCreated] = useState<string | null>(null);
  const [checking, setChecking] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [heard, setHeard] = useState(0);
  const attach = useAttach("quick");
  const { open, task, ask, ask_scope: scope } = draft;
  // The page's feed says when this thread moves, the chat's recipient changes or a stream opens
  // again; the open chat reads its thread then.
  useEffect(() => {
    if (!open || task || ask <= 0) return;
    const moved = (event: Event) => {
      if (concernsThread((event as Event & { moved?: unknown }).moved, ask)) {
        setHeard((count) => count + 1);
      }
    };
    addEventListener("review:asks", moved);
    return () => removeEventListener("review:asks", moved);
  }, [open, task, ask]);
  // Opened from its button, the sheet takes the focus at the field the reader types in.
  useEffect(() => {
    if (open) takeFocus("quick", QUICK_FIRST);
  }, [open]);
  useEscape("#quick-sheet", () => {
    change(() => ({ open: false }));
    returnFocus("#quick-open");
  });
  // The focus a send held in the sheet, given to the field the reader types in once the send is
  // over and its fields are enabled again.
  const held = useRef<Element | null>(null);
  useEffect(() => {
    if (sending) return;
    releaseFocus(held.current, QUICK_FIRST);
    held.current = null;
  }, [sending]);
  // The conversation, read when it opens and as the feed says it moved, its read given up when
  // another replaces it; a failed read is tried again unless the conversation is gone.
  useEffect(() => {
    void revision;
    void heard;
    if (!open || task || ask === 0) return;
    const reading = new AbortController();
    setReadError(null);
    void readThread(scoped(scope, `/api/ask?format=blocks&id=${ask}`), reading.signal, {
      thread: setThread,
      trouble: setReadError,
      missing: () => {
        setThread(null);
        change((now) =>
          now.ask === ask && now.ask_scope === scope ? { ask: 0, ask_scope: "" } : {},
        );
      },
    });
    return () => reading.abort();
  }, [open, task, ask, scope, revision, heard, change]);

  /** The draft `sent` posted: a task's save, asked again while its outcome is unknown, a reply to
   * the conversation, or a new conversation about the page. */
  const deliver = async (sent: Draft): Promise<unknown> => {
    if (sent.task) {
      const page = taskPage(sent, route, location);
      // A reload sends the same request, for the same page.
      change(() => ({ request_page: page }));
      const slug = await saveTask(
        scoped(sent.request_scope, "/api/quick-task"),
        {
          request: sent.request,
          title: sent.title,
          body: attach.compose(sent.text),
          slug: sent.slug,
          priority: priorityOf(sent.priority),
          page,
        },
        mount,
        setChecking,
      );
      return slug === null ? null : { slug };
    }
    if (sent.ask > 0) {
      return post(scoped(sent.ask_scope, "/api/ask-reply"), {
        id: sent.ask,
        text: attach.compose(sent.text),
      });
    }
    const ask = askOf(route, sent.text, location);
    return post(scoped(route.scope, "/api/ask"), {
      ...ask,
      question: attach.compose(ask.question),
    });
  };

  const send = (event: FormEvent) => {
    event.preventDefault();
    if (!canSend(draft, sending, attach)) return;
    if (draft.task && titleLength(draft.title) > QUICK_TASK_TITLE_MAX) {
      setError(`Use at most ${QUICK_TASK_TITLE_MAX} characters for the task title`);
      return;
    }
    setSending(true);
    setError(null);
    setChecking(null);
    let sent = draft;
    if (draft.task) {
      setCreated(null);
      if (draft.request === "") {
        let request: string;
        try {
          request = requestIdentity();
        } catch (failure) {
          setError(said(failure));
          setSending(false);
          return;
        }
        sent = { ...draft, request, request_scope: route.scope };
        change(() => ({ request, request_scope: route.scope }));
      }
    }
    held.current = holdFocus("#quick-sheet", SHOWN_PANEL);
    void (async () => {
      try {
        const value = await deliver(sent);
        if (mount.aborted) return;
        attach.clear();
        let saved = false;
        change((now) => {
          saved = now.task;
          return afterSent(now, value, route.scope);
        });
        if (saved) {
          const slug = field(value, "slug");
          setCreated(typeof slug === "string" ? slug : null);
        }
        setRevision((count) => count + 1);
      } catch (failure) {
        if (mount.aborted) return;
        change(() => ({ request: "", request_page: "", request_scope: "" }));
        if (
          !sent.task &&
          sent.ask > 0 &&
          failure instanceof Failure &&
          failure.kind === "missing"
        ) {
          change((now) =>
            now.ask === sent.ask && now.ask_scope === sent.ask_scope
              ? { ask: 0, ask_scope: "" }
              : {},
          );
          setThread(null);
          setReadError(null);
          setError(
            "This conversation is no longer available. Your text is kept; send again to start a new conversation.",
          );
        } else {
          setError(said(failure));
        }
      }
      setChecking(null);
      setSending(false);
    })();
  };

  const busy = sending || draft.request !== "";
  const shown = thread !== null && thread.ask.id === draft.ask ? thread : null;
  const content = (
    <>
      <QuickThread
        draft={draft}
        shown={shown}
        readError={readError}
        checking={checking}
        created={created}
      />
      {/* Out of the thread, so a conversation that lands never moves it. */}
      {!draft.task && draft.ask > 0 && (
        <div className="quick-restart">
          <Button
            type="button"
            disabled={sending}
            onClick={() => {
              change(() => ({ ask: 0, ask_scope: "" }));
              setThread(null);
              setReadError(null);
            }}
          >
            New conversation
          </Button>
        </div>
      )}
      <form onSubmit={send} data-attach={attach.zone}>
        {draft.task && (
          <>
            <label htmlFor="quick-title">Title</label>
            <Input
              id="quick-title"
              value={draft.title}
              required
              maxLength={QUICK_TASK_TITLE_MAX}
              aria-describedby="quick-title-limit"
              disabled={busy}
              onChange={(event) => change(() => ({ title: event.target.value }))}
            />
            <p id="quick-title-limit" role="status">
              {`${titleLength(draft.title)} / ${QUICK_TASK_TITLE_MAX} characters`}
            </p>
          </>
        )}
        <label htmlFor="quick-text">{draft.task ? "Details" : "Message"}</label>
        <Textarea
          id="quick-text"
          rows={3}
          value={draft.text}
          required={!draft.task}
          maxLength={4000}
          disabled={busy}
          onChange={(event) => change(() => ({ text: event.target.value }))}
        />
        {draft.task && (
          <div className="row">
            {/* biome-ignore lint/a11y/noLabelWithoutControl: the field it labels, an Input, is inside it */}
            <label>
              Slug (optional)
              <Input
                value={draft.slug}
                disabled={busy}
                onChange={(event) => change(() => ({ slug: event.target.value }))}
              />
            </label>
            {/* biome-ignore lint/a11y/noLabelWithoutControl: the field's trigger, a button, is the control it labels */}
            <label>
              Priority
              <SelectField
                label="Priority"
                value={draft.priority}
                disabled={busy}
                options={PRIORITIES}
                onChange={(priority) => change(() => ({ priority }))}
              />
            </label>
          </div>
        )}
        <AttachControls attach={attach} disabled={busy} />
        {error !== null && <p role="alert">{error}</p>}
        <Button type="submit" disabled={!canSend(draft, sending, attach)}>
          {sending ? "Sending…" : draft.request !== "" ? "Check save" : "Send"}
        </Button>
      </form>
    </>
  );
  return (
    <aside
      id="quick-sheet"
      hidden={!draft.open}
      aria-label="Chat or add a task"
      data-busy={busy || draft.title !== "" || draft.text !== "" || !attach.empty || attach.busy}
      data-posting={sending}
    >
      <Tabs
        disabled={busy}
        value={draft.task ? "task" : "chat"}
        values={TABS}
        horizontal
        onChange={(value) => change(() => ({ task: value === "task" }))}
      >
        <div className="tabs">
          <TabList>
            <TabTrigger id="quick-chat-tab" value="chat" controls="quick-chat-panel">
              Chat
            </TabTrigger>
            <TabTrigger id="quick-task-tab" value="task" controls="quick-task-panel">
              Add task
            </TabTrigger>
          </TabList>
          {/* The button sits at the window's right edge: its tip opens leftwards, over the sheet. */}
          <Tooltip
            side="left"
            trigger={(props) => (
              <button
                {...props}
                className={`${props.className} collapse`}
                type="button"
                aria-label="Collapse panel"
                onClick={() => {
                  change(() => ({ open: false }));
                  returnFocus("#quick-open");
                }}
              >
                <svg
                  aria-hidden="true"
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="m6 9 6 6 6-6" />
                </svg>
              </button>
            )}
          >
            Collapse panel; keep the conversation
          </Tooltip>
        </div>
        <TabContent id="quick-chat-panel" value="chat" labelledBy="quick-chat-tab">
          {content}
        </TabContent>
        <TabContent id="quick-task-panel" value="task" labelledBy="quick-task-tab">
          {content}
        </TabContent>
      </Tabs>
    </aside>
  );
}
