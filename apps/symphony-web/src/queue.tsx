// The complete review queue: what waits on the owner, by the document folders' topics, then what
// to spot-check and what waits on an agent. The board's feed invalidates it; rows mount their
// contents near the viewport, while each group's anchor stays for the home's links.

import { useEffect, useRef, useState } from "react";
import { useFreshen } from "./board-page";
import { Button } from "./button";
import { answered, boardItem, drawn, failed, keepLater, QUEUE, readKept } from "./cache";
import { PostponeControls } from "./postpone";
import { href, scoped } from "./project";
import { getIf, said } from "./request";
import { scrollAnchor } from "./route";
import { field, list } from "./shared";
import { Backoff, NEVER_ANSWERED } from "./state";
import { load, save } from "./storage";
import { TaskTag } from "./task-tag";
import { retryWait, tick } from "./timing";
import { failed as failedRead, ready } from "./trouble";
import { drawnHeight, drawnStyle, NearViewport, rememberDrawn } from "./viewport";
import { ASK_LATE_SECONDS, type Card, type Queue, type QueueAsk, type QueueItem } from "./wire";

/** The feed's regions that move the queue: its items and their order, in each of its groups. */
const REGIONS = [
  "queue-item",
  "queue-item-order",
  "spot-check",
  "spot-check-order",
  "agent-queue",
  "agent-queue-order",
];

/** Whether a feed event moves the queue. */
export function changed(delta: unknown): boolean {
  return list(field(delta, "changes")).some((change) =>
    REGIONS.includes(field(change, "region") as string),
  );
}

const isText = (value: unknown): value is string => typeof value === "string";
const isOptionalText = (value: unknown) => value === undefined || value === null || isText(value);

function isCard(value: unknown): value is Card {
  return (
    ["slug", "doc", "doc_title", "title", "why"].every((key) => isText(field(value, key))) &&
    ["posted", "task"].every((key) => isOptionalText(field(value, key))) &&
    [undefined, true, false].includes(field(value, "postponed") as boolean | undefined)
  );
}

const isOptionalNumber = (value: unknown) =>
  value === undefined || value === null || typeof value === "number";

/** An item's ask: none, or its number, when it was said and, when held, by whom and since when. */
function isOptionalAsk(value: unknown): boolean {
  if (value === undefined || value === null) return true;
  return (
    ["id", "asked"].every((key) => typeof field(value, key) === "number") &&
    isOptionalText(field(value, "holder")) &&
    isOptionalNumber(field(value, "held"))
  );
}

function isItem(value: unknown): value is QueueItem {
  return (
    ["href", "path", "slug", "title", "kind", "text"].every((key) => isText(field(value, key))) &&
    isOptionalText(field(value, "branch")) &&
    isOptionalAsk(field(value, "ask"))
  );
}

/** `seconds` as `just tasks asks` says them: whole minutes, and hours past the hour. */
export function minutes(seconds: number): string {
  const whole = Math.floor(Math.max(0, seconds) / 60);
  const [hours, rest] = [Math.floor(whole / 60), whole % 60];
  if (hours === 0) return `${rest} min`;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

/** Whether an ask said at `asked` is late at `now`, both in seconds since the epoch. */
export function askLate(asked: number, now: number): boolean {
  return now - asked >= ASK_LATE_SECONDS;
}

/**
 * An open ask at `now` (seconds since the epoch), as `just tasks asks` says one: how long it has
 * waited, late past the board's bound, and who holds it and for how long, nobody said as that.
 */
export function askLine(ask: QueueAsk, now: number): string {
  const age = `${minutes(now - ask.asked)}${askLate(ask.asked, now) ? ", late" : ""}`;
  const holder = ask.holder ?? null;
  const held = ask.held ?? null;
  if (holder === null) return `${age} · held by nobody`;
  return held === null
    ? `${age} · held by ${holder}`
    : `${age} · held by ${holder} for ${minutes(now - held)}`;
}

/** The line of a row whose card has an open ask on the board; `data-late` once it is late. */
export function AskLine({ ask, now }: { ask: QueueAsk; now: number }) {
  return (
    <p className="queue-ask muted" data-ask={ask.id} data-late={askLate(ask.asked, now)}>
      {askLine(ask, now)}
    </p>
  );
}

/**
 * The queue as this client reads it, or none for a value without what the page draws: the server's
 * answer and the copy this device kept alike, as Rust's serde refuses one. A field the page does
 * not draw is the server's to add or leave out (`branches`, a card's summary and pictures).
 */
export function readQueue(value: unknown): Queue | null {
  const [waiting, spot, agent] = ["waiting", "spot_check", "agent"].map((key) => field(value, key));
  return Array.isArray(waiting) &&
    waiting.every(isCard) &&
    [spot, agent].every((group) => Array.isArray(group) && group.every(isItem))
    ? (value as Queue)
    : null;
}

/** The topic a card's document gives it in the queue: the folder under `docs/`, else `docs`. */
export function queueCategory(path: string): string {
  let rest = path;
  if (rest.startsWith("@")) {
    const slash = rest.indexOf("/");
    rest = slash < 0 ? rest : rest.slice(slash + 1);
  }
  if (!rest.startsWith("docs/")) return "docs";
  const inside = rest.slice("docs/".length);
  const slash = inside.indexOf("/");
  return slash < 0 ? "docs" : inside.slice(0, slash);
}

/** How many of `paths` each topic holds, by topic in order. */
export function counts(paths: string[]): Map<string, number> {
  const found = new Map<string, number>();
  for (const path of paths) {
    const topic = queueCategory(path);
    found.set(topic, (found.get(topic) ?? 0) + 1);
  }
  return new Map([...found].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
}

/** A topic chosen before that this queue no longer has shows all, not an empty group. */
export function active(selected: string, topics: Map<string, number>): string {
  return topics.has(selected) ? selected : "";
}

/** The cards of what waits on the owner its group shows with the topic `selected`. */
export function visible(items: Card[], selected: string): Card[] {
  const topic = active(selected, counts(items.map((card) => card.doc)));
  return items.filter((card) => topic === "" || queueCategory(card.doc) === topic);
}

/** Where this device keeps the topic chosen in what waits on the owner. */
const CATEGORY = "review:queue:you:category";

/** The topic this device chose in what waits on the owner; all (empty) when it chose none. */
export function chosenTopic(): string {
  return load(CATEGORY) ?? "";
}

/** Keep `topic` as this device's choice; empty for all. */
export function chooseTopic(topic: string): void {
  save(CATEGORY, topic);
}

/**
 * Where this browser keeps how many rows each group showed when the queue last drew, as `WAITING
 * SPOT AGENT`; the groups' heights are kept as `queue:ANCHOR` (`drawnHeight`).
 */
const ROWS = "queue-rows";

/** The rows `kept` (as ROWS holds them) gives each group, none for one it does not say. */
export function recallRows(kept: string): (number | null)[] {
  const rows = kept.split(/\s+/).filter((word) => word !== "");
  return [0, 1, 2].map((at) => {
    const word = rows[at];
    // As Rust reads a count: digits, an optional `+` before them.
    return word !== undefined && /^\+?\d+$/.test(word) ? Number(word) : null;
  });
}

/** The rows each group showed, as ROWS keeps them. */
export function keepRows([waiting, spot, agent]: [number, number, number]): string {
  return `${waiting} ${spot} ${agent}`;
}

/** The queue's groups in their order: each one's anchor, and the rows it is guessed to show. */
const GROUPS: [string, number][] = [
  ["you", 4],
  ["spot", 2],
  ["agent", 2],
];

/** A row of the queue while it loads; a card waiting on the owner has its rank and Later too. */
function GhostRow({ ranked = false }: { ranked?: boolean }) {
  return (
    <div className="ghost-row">
      {ranked && <span className="ghost-rank">{" "}</span>}
      <div>
        <span className="bone" style={{ "--w": "66%" } as React.CSSProperties} />
        <div>
          <span className="bone" style={{ "--w": "44%" } as React.CSSProperties} />
        </div>
        {ranked && <span className="ghost-field ghost-later" />}
      </div>
    </div>
  );
}

/**
 * The queue before its read: each group's card with its heading and as many rows as it showed
 * last, at the height it took in a window as wide; else the rows it is likely to hold.
 */
function GhostQueue() {
  const rows = recallRows(load(ROWS) ?? "");
  return (
    <>
      {GROUPS.map(([id, guess], group) => (
        <div
          key={id}
          className="card ghost-group"
          data-for={id}
          style={drawnStyle(drawnHeight(`queue:${id}`))}
        >
          <div>
            <span className="bone" style={{ "--w": "12ch" } as React.CSSProperties} />
          </div>
          {/* What waits on the owner opens on its topic chips. */}
          {group === 0 && (
            <div className="ghost-chips">
              <span className="ghost-pill">
                <span className="bone" style={{ "--w": "4ch" } as React.CSSProperties} />
              </span>
              <span className="ghost-pill">
                <span className="bone" style={{ "--w": "6ch" } as React.CSSProperties} />
              </span>
            </div>
          )}
          {/* A group that showed none says so in a line. */}
          {rows[group] === 0 && (
            <div className="ghost-none">
              <span className="bone" style={{ "--w": "14ch" } as React.CSSProperties} />
            </div>
          )}
          {Array.from({ length: Math.min(rows[group] ?? guess, 12) }, (_, n) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: the ghost rows are alike
            <GhostRow key={n} ranked={group === 0} />
          ))}
        </div>
      ))}
    </>
  );
}

/** The topic chips of what waits on the owner: All, then each topic with its count. */
export function CategoryChips({
  topics,
  selected,
  onSelect,
}: {
  topics: Map<string, number>;
  selected: string;
  onSelect: (topic: string) => void;
}) {
  const chosen = active(selected, topics);
  const total = [...topics.values()].reduce((sum, count) => sum + count, 0);
  return (
    // biome-ignore lint/a11y/useSemanticElements: a group of toggles, as the Dioxus page draws it
    <div className="queue-categories" role="group" aria-label="Review categories">
      <Button size="sm" aria-pressed={chosen === ""} data-category="" onClick={() => onSelect("")}>
        {`All (${total})`}
      </Button>
      {[...topics].map(([topic, count]) => (
        <Button
          key={topic}
          size="sm"
          aria-pressed={chosen === topic}
          data-category={topic}
          onClick={() => onSelect(topic)}
        >
          {`${topic} (${count})`}
        </Button>
      ))}
    </div>
  );
}

/** What waits on the owner, ranked, with the topic chips and each card's Later. */
export function WaitingQueue({
  items,
  selected,
  onSelect,
}: {
  items: Card[];
  selected: string;
  onSelect: (topic: string) => void;
}) {
  const topics = counts(items.map((card) => card.doc));
  const shown = visible(items, selected);
  const chosen = active(selected, topics);
  return (
    <section id="you" className="card queue-group">
      <h2>{`Waiting on you (${items.length})`}</h2>
      <CategoryChips topics={topics} selected={chosen} onSelect={onSelect} />
      {chosen !== "" && (
        <p className="queue-filter-count muted" role="status">
          {`Showing ${shown.length} of ${items.length}`}
        </p>
      )}
      <ol className="queue-list ranked">
        {items.length === 0 && <li className="muted">nothing to decide</li>}
        {shown.map((card, index) => (
          <li key={`${card.doc}#${card.slug}`} data-card={`${card.doc}#${card.slug}`}>
            <NearViewport height={64} ghost={<GhostRow ranked />}>
              <div className="queue-row">
                <span className="queue-rank">{index + 1}</span>
                <div>
                  <a href={href(`/${card.doc}#${card.slug}`)}>{card.title}</a>
                  <p className="muted">{`${card.doc_title} · ${card.why}`}</p>
                  <TaskTag task={card.task} />
                  <PostponeControls
                    path={card.doc}
                    slug={card.slug}
                    postponed={card.postponed === true}
                  />
                </div>
              </div>
            </NearViewport>
          </li>
        ))}
      </ol>
    </section>
  );
}

/**
 * A group of the queue below what waits on the owner: to spot-check, or waiting on an agent, where
 * a card with an open ask on the board says how long it has waited and who holds it.
 */
export function QueueGroup({
  id,
  title,
  items,
  empty,
  now,
}: {
  id: string;
  title: string;
  items: QueueItem[];
  empty: string;
  /** Now, in seconds since the epoch: the moment each row's ask is aged at. */
  now: number;
}) {
  return (
    <section id={id} className="card queue-group">
      <h2>{`${title} (${items.length})`}</h2>
      <ul className="queue-list">
        {items.length === 0 && <li className="muted">{empty}</li>}
        {items.map((item) => (
          <li key={item.href} data-card={`${item.path}#${item.slug}`}>
            <NearViewport height={72} ghost={<GhostRow />}>
              <div className="queue-row">
                <div>
                  <a href={href(item.href)}>{item.title}</a>
                  {item.branch !== undefined && item.branch !== null ? (
                    <p className="muted">
                      <span className="chip">{item.branch}</span>
                      {` ${item.path}`}
                    </p>
                  ) : (
                    <>
                      {item.kind !== "" && <span className="chip">{item.kind}</span>}
                      {item.text !== "" && <p className="muted">{item.text}</p>}
                    </>
                  )}
                  {item.ask !== undefined && item.ask !== null && (
                    <AskLine ask={item.ask} now={now} />
                  )}
                </div>
              </div>
            </NearViewport>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** What a check of the queue tells the page. */
export interface QueueCheck {
  /** The queue answered, with its tag and the value as read; none for a `304` of the copy shown. */
  answered(read: Queue | null, etag: string, value: unknown): void;
  /** A read failed, or answered what is not a queue: said, and asked again after a wait. */
  failed(said: string): void;
}

/**
 * Reads the queue at `url`, naming the tag of the copy shown (`etag`), until it answers or `signal`
 * ends it: a read that fails, or whose answer the guard refuses, is asked again, waiting longer
 * each time (`Backoff`, through `wait`).
 */
export async function checkQueue(
  url: string,
  etag: () => string,
  signal: AbortSignal,
  check: QueueCheck,
  wait: typeof retryWait = retryWait,
): Promise<void> {
  const backoff = new Backoff();
  while (!signal.aborted) {
    try {
      const answer = await getIf(url, etag(), signal);
      if (signal.aborted) return;
      if (answer.same) {
        check.answered(null, "", null);
      } else {
        const read = readQueue(answer.value);
        if (read === null) throw new Error("The queue's answer is not a queue");
        check.answered(read, answer.etag, answer.value);
      }
      return;
    } catch (failure) {
      if (signal.aborted) return;
      check.failed(said(failure));
    }
    await wait(backoff, NEVER_ANSWERED, signal);
  }
}

/**
 * The queue: the copy this device kept is drawn at once, and checked once the board's read has
 * started the feed (`refreshed` counts the board's reads; the feed's queue events add to it).
 */
export function QueuePage({ scope, refreshed }: { scope: string; refreshed: number }) {
  const [queue, setQueue] = useState<Queue | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(chosenTopic);
  const [located, setLocated] = useState(false);
  const etag = useRef("");
  const shownQueue = useRef<Queue | null>(null);
  const freshen = useFreshen();
  // The asks' ages are read again each minute.
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const controller = new AbortController();
    void (async () => {
      while (!controller.signal.aborted) {
        await tick(controller.signal);
        if (!controller.signal.aborted) setNow(Date.now() / 1000);
      }
    })();
    return () => controller.abort();
  }, []);
  useEffect(() => {
    let live = true;
    void readKept(boardItem(scope), QUEUE, readQueue).then((kept) => {
      if (kept && live && shownQueue.current === null) {
        etag.current = kept.etag;
        shownQueue.current = kept.value;
        setQueue(kept.value);
        freshen((f) => drawn(f, QUEUE, kept.saved));
      }
    });
    return () => {
      live = false;
    };
  }, [scope, freshen]);
  useEffect(() => {
    if (refreshed === 0) return;
    const controller = new AbortController();
    const item = boardItem(scope);
    // The copy drawn from this device, and its held Later buttons, wait for the check.
    void checkQueue(scoped(scope, "/api/queue"), () => etag.current, controller.signal, {
      answered(read, tag, value) {
        if (read !== null) {
          etag.current = tag;
          keepLater(item, QUEUE, tag, value);
          if (JSON.stringify(shownQueue.current) !== JSON.stringify(read)) {
            shownQueue.current = read;
            setQueue(read);
          }
        }
        setError(null);
        freshen((f) => answered(f, QUEUE));
      },
      failed(why) {
        freshen((f) => failed(f, QUEUE));
        setError(why);
      },
    });
    return () => controller.abort();
  }, [refreshed, scope, freshen]);
  // The heights are measured while the page stays, whatever it draws again meanwhile.
  const [mounted] = useState(() => new AbortController());
  useEffect(() => () => mounted.abort(), [mounted]);
  useEffect(() => {
    if (queue === null || located) return;
    setLocated(true);
    scrollAnchor();
    // What each group showed, and its height once its rows near the screen drew, for the ghost of
    // the next visit.
    save(
      ROWS,
      keepRows([
        visible(queue.waiting, selected).length,
        queue.spot_check.length,
        queue.agent.length,
      ]),
    );
    void rememberDrawn(
      GROUPS.map(([id]) => [`queue:${id}`, `#${id}`]),
      500,
      mounted.signal,
    );
  }, [queue, located, selected, mounted]);
  return (
    <div
      data-ready={located ? "queue" : ready(false, error !== null)}
      aria-busy={queue === null && error === null}
    >
      <div className="dash queue-page">
        <div className="page-head">
          <div>
            <p className="eyebrow">
              <a href={href(scoped(scope, "/"))}>← Home</a>
            </p>
            <h1>Review queue</h1>
          </div>
        </div>
        {error !== null && <p role="alert">{failedRead(error, true)}</p>}
        {queue !== null ? (
          <>
            <WaitingQueue
              items={queue.waiting}
              selected={selected}
              onSelect={(topic) => {
                chooseTopic(topic);
                setSelected(topic);
              }}
            />
            <QueueGroup
              id="spot"
              title="Auto-approved: spot-check"
              items={queue.spot_check}
              empty="none"
              now={now}
            />
            <QueueGroup
              id="agent"
              title="Waiting on the agent"
              items={queue.agent}
              empty="every comment is answered"
              now={now}
            />
          </>
        ) : (
          error === null && (
            <>
              <p role="status" className="status-text">
                Loading the queue…
              </p>
              <GhostQueue />
            </>
          )
        )}
      </div>
    </div>
  );
}
