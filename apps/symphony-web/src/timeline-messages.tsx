import type { JSX, ReactNode } from "react";
import { Button } from "./button";
import { writer } from "./project";
import { address, endpointWords, historyWords, isoTime } from "./timeline-history";
import type { TaskEntry, TaskTimeline, TimelineMessage } from "./wire";

/** The task page owns the thread, its reads and its rendering; the timeline holds no bodies. */
export interface ThreadAccess {
  entries: readonly TaskEntry[];
  before: number | null;
  pending: boolean;
  error: string | null;
  earlier(): Promise<string | null>;
  latest(): Promise<string | null>;
  render(entry: TaskEntry): ReactNode;
}

export interface MessageSelection {
  messages: TimelineMessage[];
  selected: number | null;
  choose(id: number): void;
}

export function MessagePanel({
  timeline,
  selection,
  thread,
  loading,
  error,
  retry,
  earlier,
}: {
  timeline: TaskTimeline;
  selection: MessageSelection;
  thread?: ThreadAccess | undefined;
  loading: boolean;
  error: string | null;
  retry(): void;
  earlier(): void;
}): JSX.Element {
  const { messages, selected, choose } = selection;
  const index = messages.findIndex((m) => m.id === selected);
  const message = messages[index];
  const entry = thread?.entries.find((e) => e.message.id === selected);
  return (
    <section className="tl-message-panel" aria-label="Timeline messages">
      <p className="tl-note">{historyWords(timeline.history)}</p>
      <div className="tl-message-controls">
        <label htmlFor="tl-message-choice">Messages, in send order</label>
        <select
          id="tl-message-choice"
          disabled={messages.length === 0}
          value={selected ?? ""}
          onChange={(e) => choose(Number(e.currentTarget.value))}
          onKeyDown={(event) => {
            const direction = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : 0;
            const next = messages[index + direction];
            if (direction && next) {
              event.preventDefault();
              choose(next.id);
            }
          }}
        >
          <option value="" disabled>
            Select a message
          </option>
          {messages.map((m) => (
            <option key={m.id} value={m.id}>{`#${m.id} · ${address(m)}`}</option>
          ))}
        </select>
        <Button
          disabled={index <= 0}
          onClick={() => {
            const m = messages[index - 1];
            if (m) choose(m.id);
          }}
        >
          Previous message
        </Button>
        <Button
          disabled={index >= messages.length - 1}
          onClick={() => {
            const m = messages[index + 1];
            if (m) choose(m.id);
          }}
        >
          Next message
        </Button>
      </div>
      {messages.length === 0 && <p>No recorded messages in the available range.</p>}
      {error && !message && <p role="alert">{error}</p>}
      {message && (
        <div className="tl-message-details" aria-live="polite">
          <h3>{`Message #${message.id} · ${address(message)}`}</h3>
          <p>
            <time dateTime={isoTime(message.at) ?? "Calendar time unavailable"}>
              {isoTime(message.at) ?? "Calendar time unavailable"}
            </time>
            {` · recorded time ${message.at}`}
          </p>
          <p>{`Sender: ${writer(message.sender).text}. ${endpointWords(message.from)}`}</p>
          <p>
            {`Addressed to: ${message.recipient == null ? "Everyone on task" : writer(message.recipient).text}. ${endpointWords(message.to)}`}
          </p>
          <p>
            {message.after_close === true
              ? "Sent after task closure."
              : message.after_close === null
                ? "Closure state at send time is unknown."
                : "Sent while the task was open."}{" "}
            Arrows mean addressed to; delivery, reading and reply causality are not recorded.
          </p>
          {entry ? (
            thread?.render(entry)
          ) : (
            <p role="status">
              {error ??
                (loading || thread?.pending
                  ? "Loading message from the task thread…"
                  : (thread?.error ?? "Message text is not in the loaded thread."))}
              {!loading && !thread?.pending && (
                <Button onClick={retry}>
                  {error || thread?.error ? "Retry message" : "Load message"}
                </Button>
              )}
            </p>
          )}
          <a href={`#message-${message.id}`}>Original thread entry</a>
        </div>
      )}
      {thread?.before !== null && thread !== undefined && (
        <Button disabled={loading} onClick={earlier}>
          Load earlier messages
        </Button>
      )}
    </section>
  );
}
