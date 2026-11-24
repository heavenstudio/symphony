// Home (`/`): the greeting and the page links, the next card with its pictures and the four after
// it, the counts, and the progress the project's plugins add with the design documents'. The board
// it draws is kept current by the feed (home-board.ts); each region draws again only when the event
// changed its own part of the board. The Dioxus client's view.rs, drawn alike.

import { memo, useEffect, useState } from "react";
import { openActivity } from "./frame";
import { PictureView } from "./media-picture";
import { documentsHref, href, project } from "./project";
import { TaskTag } from "./task-tag";
import { tick } from "./timing";
import type {
  Board,
  Card,
  Counts,
  Picture,
  Progress,
  ProgressBar,
  ProjectView,
  Region,
} from "./wire";
import { fixed, localMoment, parseMoment, plural } from "./words";

/** How many pictures the hero shows: a picture card's candidates, or the items a card names. */
const HERO_PICTURES = 6;

/** "Good morning", "Good afternoon" or "Good evening", by the hour of `now` (a moment). */
export function greeting(now: number): string {
  const hour = Math.floor((((now % 1440) + 1440) % 1440) / 60);
  return hour <= 11 ? "Good morning" : hour <= 17 ? "Good afternoon" : "Good evening";
}

/** How long something posted has waited at `now`, short: "new", "3 h", "7 d"; "" without a time. */
export function age(posted: string | null | undefined, now: number): string {
  const then = posted == null ? null : parseMoment(posted);
  if (then === null) return "";
  const minutes = Math.max(now - then, 0);
  if (minutes >= 1440) return `${Math.floor(minutes / 1440)} d`;
  if (minutes >= 60) return `${Math.floor(minutes / 60)} h`;
  return "new";
}

/** The line under the greeting. */
export function lead(waiting: number, docs: number): string {
  if (waiting === 0) return "Your queue is empty.";
  const waits = waiting === 1 ? "card waits" : "cards wait";
  return `${waiting} ${waits} on you across ${docs} ${plural(docs, "document")}.`;
}

/** `part` of `whole` in percent; 0 of nothing. */
function percent(part: number, whole: number): number {
  return whole === 0 ? 0 : (100 * part) / whole;
}

/** The moment it is where the owner is, read again each minute. */
function useNow(): number {
  const [now, setNow] = useState(() => localMoment(new Date()));
  useEffect(() => {
    const controller = new AbortController();
    void keepTime(controller.signal, setNow);
    return () => controller.abort();
  }, []);
  return now;
}

/** The moment it is where the owner is, told to `shown` at each minute's tick until `signal` ends. */
export async function keepTime(signal: AbortSignal, shown: (now: number) => void): Promise<void> {
  while (!signal.aborted) {
    await tick(signal);
    if (!signal.aborted) shown(localMoment(new Date()));
  }
}

/** The body of `/`: the board drawn at `now` for `view`'s project. */
export function BoardView({
  board,
  now,
  view = project(),
}: {
  board: Board;
  now: number;
  view?: ProjectView;
}) {
  const counts = board.counts;
  const [first, ...rest] = board.next;
  return (
    <div className="dash">
      <div className="dash-head page-head">
        <div>
          <h1>{greeting(now)}</h1>
          <p className="head-state">{lead(counts.waiting, counts.docs_waiting)}</p>
        </div>
        <p className="dash-links">
          <a className="pill" href={href("/queue")}>
            Full queue
          </a>
          <a className="pill" href={href(documentsHref(view))}>
            All docs
          </a>
          {view.nav.map((item) => (
            <a key={item.href} className="pill" href={href(item.href)}>
              {item.title}
            </a>
          ))}
          {/* The register and the guide are the project's: a project without either file (a new
              one) has no link to it. */}
          {view.register !== "" && (
            <a className="pill" href={href(`/${view.register}`)}>
              Decision register
            </a>
          )}
          {(view.guide ?? "") !== "" && (
            <a className="pill" href={href(`/${view.guide}`)}>
              How review works
            </a>
          )}
        </p>
      </div>
      <div className="dash-top">
        <Hero card={first} waiting={counts.waiting} spot={counts.spot_check} now={now} />
        {rest.length > 0 && <NextList cards={rest} waiting={counts.waiting} />}
      </div>
      <Tiles counts={counts} />
      <div className="dash-grid">
        <ProgressPanel progress={board.progress} docs={documentsHref(view)} />
      </div>
    </div>
  );
}

/** The next card, with a link straight to it; or the quiet of an empty queue. */
const Hero = memo(function Hero({
  card,
  waiting,
  spot,
  now,
}: {
  card: Card | undefined;
  waiting: number;
  spot: number;
  now: number;
}) {
  if (card === undefined) {
    return (
      <section className="hero card empty">
        <div className="hero-text">
          <p className="eyebrow">Next up</p>
          <h2 className="hero-title">Nothing waits on you. ✦</h2>
          {spot > 0 && (
            <p className="hero-summary">
              <a href={href("/queue#spot")}>{`${spot} auto-approved ${plural(spot, "card")}`}</a>
              {" to spot-check."}
            </p>
          )}
        </div>
      </section>
    );
  }
  const waited = age(card.posted, now);
  const pictures = card.pictures ?? [];
  return (
    <section className="hero card">
      <div className="hero-text">
        <p className="eyebrow">{`Next up · 1 of ${waiting}`}</p>
        <h2 className="hero-title">{card.title}</h2>
        {(card.summary ?? "") !== "" && <p className="hero-summary">{card.summary}</p>}
        <p className="hero-meta">
          <a className="docchip" href={href(`/${card.doc}`)}>
            {card.doc_title}
          </a>
          <TaskTag task={card.task} /> <span className="why">{card.why}</span>
          {waited !== "" && (
            <>
              {" "}
              <span className="muted">{`waiting ${waited}`}</span>
            </>
          )}
        </p>
        <a className="go" href={href(`/${card.doc}#${card.slug}`)}>
          {"Open this card "}
          <span aria-hidden="true">→</span>
        </a>
      </div>
      {pictures.length > 0 && (
        <div className="pics">
          {pictures.slice(0, HERO_PICTURES).map((picture, at) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: the card's pictures, in its order
            <Figure key={at} picture={picture} />
          ))}
        </div>
      )}
    </section>
  );
});

function Figure({ picture }: { picture: Picture }) {
  if (picture.src == null) {
    return (
      <figure className="none">
        <div className="nopic">picture coming</div>
        <figcaption>{picture.caption}</figcaption>
      </figure>
    );
  }
  return (
    <figure className={picture.waiting === true ? "waiting" : undefined}>
      <PictureView src={picture.src} caption={picture.caption} />
      <figcaption>{picture.caption}</figcaption>
    </figure>
  );
}

/** The cards after the next one, in the queue's order, each by its place in it: the hero is 1. */
function NextList({ cards, waiting }: { cards: Card[]; waiting: number }) {
  return (
    <section className="next card">
      <h2 className="eyebrow">Then</h2>
      <div className="nextlist">
        {cards.map((card, index) => (
          <NextCard key={`${card.doc}#${card.slug}`} card={card} rank={index + 2} />
        ))}
      </div>
      <a className="more" href={href("/queue")}>
        {`show all ${waiting} →`}
      </a>
    </section>
  );
}

/** A card of the list, its first picture as its thumbnail, else its place in the queue. */
const NextCard = memo(function NextCard({ card, rank }: { card: Card; rank: number }) {
  const thumb = (card.pictures ?? []).find((picture) => picture.src != null)?.src;
  return (
    <a className="nextcard" href={href(`/${card.doc}#${card.slug}`)}>
      {thumb != null ? (
        <PictureView src={thumb} caption="" thumb />
      ) : (
        <span className="thumb glyph">{rank}</span>
      )}
      <span className="nexttext">
        <strong>{card.title}</strong>
        <small>{`${card.doc_title} · ${card.why}`}</small>
        {card.task != null && <small className="chip">{card.task}</small>}
      </span>
    </a>
  );
});

/** The row of counts, each linking to where its detail lives. */
const Tiles = memo(function Tiles({ counts }: { counts: Counts }) {
  const tasks = counts.tasks;
  const waitingHint =
    counts.waiting > 0
      ? `across ${counts.docs_waiting} ${plural(counts.docs_waiting, "document")}`
      : "all clear";
  return (
    <div className="tiles">
      <Tile
        to="/queue#you"
        value={String(counts.waiting)}
        label="waiting on you"
        tone="you"
        hint={waitingHint}
      />
      <Tile
        to="/queue#spot"
        value={String(counts.spot_check)}
        label="auto-approved to spot-check"
        tone="auto"
      />
      {/* Today's replies are listed in the activity panel, no page of their own: the tile opens
          that panel, as the header's activity button does. */}
      <button
        type="button"
        className="tile card tone-answered"
        aria-controls="activity-panel"
        onClick={openActivity}
      >
        <b>{counts.answered_today}</b>
        <span>answered by agents today</span>
      </button>
      {/* The status's count: the board's In progress column leaves out its Build and Test. */}
      <Tile
        to="/tasks"
        value={tasks ? `${tasks.in_progress} · ${tasks.in_review}` : "–"}
        label="tasks in progress · in review"
        tone="tasks"
      />
      <Tile
        to="/agents"
        value={tasks ? String(tasks.agents_live) : "–"}
        label="agents live"
        tone="agents"
      />
    </div>
  );
});

/** A count and its label, with `hint` under them where a value of its own says more. */
function Tile(props: { to: string; value: string; label: string; tone: string; hint?: string }) {
  return (
    <a className={`tile card tone-${props.tone}`} href={href(props.to)}>
      <b>{props.value}</b>
      <span>{props.label}</span>
      {props.hint !== undefined && <small>{props.hint}</small>}
    </a>
  );
}

/** How many things a region's bars count in all. */
function counted(region: Region): number {
  return region.bars.reduce((sum, bar) => sum + bar.total, 0);
}

/**
 * The plugins' progress regions that have something to count, and the design documents, set apart
 * by a rule only below a region.
 */
const ProgressPanel = memo(function ProgressPanel({
  progress,
  docs,
}: {
  progress: Progress;
  docs: string;
}) {
  const waiting = progress.decisions_waiting;
  return (
    <section className="progress panel card">
      {progress.regions
        .filter((region) => counted(region) > 0)
        .map((region) => (
          <RegionView key={region.id} region={region} />
        ))}
      <div className="docsummary">
        <h3>
          {"Design docs "}
          <a className="subtle" href={href(docs)}>
            {`browse all ${progress.docs} →`}
          </a>
        </h3>
        <p>{`${waiting} ${plural(waiting, "decision")} waiting`}</p>
      </div>
    </section>
  );
});

/** A region of bars, done and waiting out of a total each, with its headline and its legend. */
function RegionView({ region }: { region: Region }) {
  const done = region.bars.reduce((sum, bar) => sum + bar.done, 0);
  const [ok, wait, empty] = region.legend;
  return (
    <div className="region" data-region={region.id}>
      <div className="headline">
        <h2>{region.title}</h2>
        <p className="big">
          {`${fixed(percent(done, counted(region)), 0)}%`}
          <small>{region.note}</small>
        </p>
      </div>
      <div className="bars">
        {region.bars.map((bar) => (
          <BarView key={bar.key} bar={bar} />
        ))}
      </div>
      <p className="legend">
        <span className="dot ok" />
        {`${ok} `}
        <span className="dot wait" />
        {`${wait} `}
        <span className="dot empty" />
        {empty}
      </p>
    </div>
  );
}

function BarView({ bar }: { bar: ProgressBar }) {
  return (
    <a className="bar" href={href(`/${bar.page}`)} title={bar.title}>
      <span className="bar-name">{bar.name}</span>
      <span className="track">
        <i className="ok" style={{ width: `${fixed(percent(bar.done, bar.total), 1)}%` }} />
        <i className="wait" style={{ width: `${fixed(percent(bar.waiting, bar.total), 1)}%` }} />
      </span>
      <span className="bar-num">
        {bar.done}
        <small>{`/${bar.total}`}</small>
      </span>
    </a>
  );
}

/** Home under its board: the board as it stands, at the minute it is where the owner is. */
export function HomePage({ board }: { board: Board }) {
  const now = useNow();
  return <BoardView board={board} now={now} />;
}
