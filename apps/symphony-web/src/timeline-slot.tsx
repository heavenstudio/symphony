// The task page's place for its timeline: a box as tall as the timeline was when this browser last
// drew it for the task (else a default), placeholder rows drawn in it, in which the timeline
// (timeline.tsx, a chunk of its own) is drawn once it has come. The box imports the chunk once it
// nears the screen and its part of the page has something to draw, behind no other read and no
// set wait, so the page's first draw carries none of the timeline's code or style, and nothing
// outside the box moves when the timeline arrives as tall as it was drawn last. The task's tokens
// are asked beside it and handed to the timeline as they came, which says their cost. On a phone the box keeps that height when a read adds a step to it; wide it grows to
// hold the whole timeline. Its style is timeline-slot.css, which the task page's kind imports.

import {
  type ComponentType,
  type CSSProperties,
  type JSX,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { trimmed } from "./gates-model";
import { PartsContext, usePart } from "./parts";
import { snapshot } from "./shared";
import { load, save } from "./storage";
import { timelinePart, tokensPart } from "./task-live";
import type { HostProps } from "./timeline";
import type { ThreadAccess } from "./timeline-messages";
import { useVisibility } from "./viewport";

/** The id of the box the timeline is drawn in. */
export const SLOT = "timeline-slot";

/**
 * The box's height on a phone (the list) for a task this browser has drawn no timeline of, in
 * pixels: the most a phone's timeline shows before its "Show all", the owner's 240
 * (docs/DECISIONS.md), so the brief and the thread start near the page's top, the earliest steps
 * that do not fit fading out.
 */
export const NARROW = 240;
/**
 * The box's height wide (the lanes, from 900 px) for a task this browser has drawn no timeline
 * of, in pixels: no most, as wide the box grows to hold the whole timeline (the owner's word on
 * inbox/timeline-should-not-be-collapsed-by-defa), but a typical whole timeline, so a first visit
 * moves the brief little when it lands: 190 to 911 px on the board's ten newest done tasks on
 * 2026-11-16, the builds and the tests each a lane of its own, 508 the median.
 */
export const WIDE = 508;
/** The least a box is reserved, whatever was kept. */
export const LEAST = 120;

/** A `u32`'s most, the most a kept height reads as. */
const MOST = 0xffff_ffff;

/** The key a task's drawn heights are kept under in this browser, before its project's id. */
export function key(slug: string): string {
  return `timeline-height:${slug}`;
}

/** A height as Rust's `str::parse::<u32>` reads it: digits, a `+` before them allowed. */
function count(text: string): number | null {
  if (!/^\+?[0-9]+$/.test(text)) return null;
  const read = Number(text);
  return read <= MOST ? read : null;
}

/**
 * The heights kept as "NARROW,WIDE" read back, each at least LEAST, the phone's at most NARROW:
 * an unreadable or missing one is the default.
 */
export function heights(kept: string | null): [number, number] {
  const said = (kept ?? "").split(",").map(trimmed);
  const read = (at: number, fallback: number, most: number) => {
    const height = count(said[at] ?? "");
    return height === null ? fallback : Math.min(Math.max(height, LEAST), most);
  };
  return [read(0, NARROW, NARROW), read(1, WIDE, MOST)];
}

/** The heights to keep once a draw took `natural` pixels at one width (`wide` or not), the other
 * kept as it was. */
export function kept(before: string | null, wide: boolean, natural: number): string {
  const [narrow, wideHeight] = heights(before);
  // As Rust's `natural.ceil().clamp(0, u32::MAX) as u32`, where nothing (NaN) is 0.
  const height = Number.isNaN(natural) ? 0 : Math.min(Math.max(Math.ceil(natural), 0), MOST);
  return wide
    ? `${narrow},${Math.max(height, LEAST)}`
    : `${Math.min(Math.max(height, LEAST), NARROW)},${wideHeight}`;
}

/** The heights this browser keeps for `slug`'s timeline: the box's, and its ghost's on the task's
 * placeholder. */
export function reserved(slug: string): [number, number] {
  return heights(load(key(slug)));
}

/** Keep the height `slug`'s timeline took now, at this width, for its next visit (the timeline
 * says it after each draw); the box shown keeps its own until the page is left. */
export function keep(slug: string, wide: boolean, natural: number): void {
  save(key(slug), kept(load(key(slug)), wide, natural));
}

/** How many placeholder rows a box `height` pixels tall draws. */
export function placeholders(height: number): number {
  return Math.min(Math.max(Math.floor(Math.max(height - 90, 0) / 46), 1), 9);
}

// The timeline's chunk, imported once for every box of the page's life.

type Host = ComponentType<HostProps>;

/** What a failed import of the timeline's chunk says in the page's notices, and its retry. */
export const SAID = "The timeline could not be loaded. ";
export const RETRY = "Retry loading the timeline";
/** The notice's id. */
const FAILURE = "timeline-error";

/** The shell's (shell/shell.ts `reviewShell`): whether the page's build is gone and the page
 * loads again, and a failed load's notice with its retry. */
interface Shell {
  gone(): Promise<boolean>;
  alarm(id: string, said: string, retry: string, again: () => void): void;
}

let host: Host | null = null;
/** The import on its way, until it has come, failed or been given up. */
let attempt: Promise<void> | null = null;
/** The boxes waiting for the chunk, each told once it has come. */
const waiting = new Set<(host: Host) => void>();

/** The chunk's host, once it has come; none before. */
export function loadedHost(): Host | null {
  return host;
}

/** No box waits any more: the page that asked has stopped, and the chunk is not fetched for it. */
const LEFT = Symbol("no box waits");

/**
 * Import the timeline's chunk, unless it has come or is on its way, and only while a box still
 * waits for it (a page that stopped meanwhile fetches nothing). Each box waiting is told when it
 * comes. One that does not come is said with its retry while a box still waits for it, after the
 * shell has said whether the page's build is gone (the page then loads again), and the next box
 * asks again. The attempt, which ends once all that is done (the one on its way, when one is).
 */
export function importTimeline(): Promise<void> {
  if (host !== null) return Promise.resolve();
  if (attempt !== null) return attempt;
  attempt = Promise.resolve()
    .then(
      async (): Promise<Host | typeof LEFT> =>
        waiting.size === 0 ? LEFT : (await import("./timeline")).TimelineHost,
    )
    .then(
      (loaded) => {
        attempt = null;
        if (loaded === LEFT) return;
        host = loaded;
        document.getElementById(FAILURE)?.remove();
        for (const told of [...waiting]) told(loaded);
      },
      async (error: unknown) => {
        attempt = null;
        if (waiting.size === 0) return;
        const shell = snapshot("reviewShell") as Shell | undefined;
        if ((await shell?.gone()) || waiting.size === 0) return;
        console.error(SAID, error);
        shell?.alarm(FAILURE, SAID, RETRY, () => void importTimeline());
      },
    );
  return attempt;
}

/** Tell `told` when the chunk has come, at once if it has (a box drawn before it came that asks
 * after); what it returns stops the wait. */
export function whenImported(told: (host: Host) => void): () => void {
  if (host !== null) {
    told(host);
    return () => {};
  }
  waiting.add(told);
  void importTimeline();
  return () => {
    waiting.delete(told);
  };
}

/** The task's timeline box, one per task: another task's page gets a new one, so its height holds
 * for the visit. `scope` is the checkout the route names. */
type SlotProps = { slug: string; scope: string; thread?: ThreadAccess | undefined };
export function TimelineSlot({ slug, scope, thread }: SlotProps): JSX.Element {
  return <SlotBox key={slug} slug={slug} scope={scope} thread={thread} />;
}

/** The box's two heights, for its style sheet to take the one its width draws. */
export function boxStyle(narrow: number, wide: number): CSSProperties {
  return { "--tl-narrow": `${narrow}px`, "--tl-wide": `${wide}px` } as CSSProperties;
}

/**
 * The box of the task `slug`: its placeholder rows until the timeline draws; the timeline's answer,
 * a part of the page asked as the box mounts (task-live.ts's `timelinePart`); the timeline's chunk,
 * imported once the box nears the screen and its part has something to draw (an answer, or why
 * there is none), behind nothing else; and the timeline drawn in it from that answer.
 */
function SlotBox({ slug, scope, thread }: SlotProps) {
  const [[narrow, wide]] = useState(() => reserved(slug));
  const [shown, watch] = useVisibility(false);
  const [details, setDetails] = useState<HTMLDivElement | null>(null);
  const [Timeline, setTimeline] = useState<Host | null>(loadedHost);
  const part = timelinePart(scope, slug);
  const parts = useContext(PartsContext);
  const answer = usePart(part);
  // The task's tokens, whose cost the timeline says after each time: asked beside the timeline,
  // drawn without until they come and where their read fails (the store tries it again).
  const tokens = usePart(tokensPart(scope, slug));
  const { key } = part;
  // biome-ignore lint/correctness/useExhaustiveDependencies: a part is its key
  const read = useCallback(() => parts?.read(part) ?? Promise.resolve(), [parts, key]);
  const some = answer.value !== undefined || answer.error !== null || answer.missing;
  useEffect(() => {
    if (!shown || !some || Timeline !== null) return;
    return whenImported((chunk) => setTimeline(() => chunk));
  }, [shown, some, Timeline]);
  const rows = placeholders(Math.max(narrow, wide));
  return (
    <section className="tl" id="timeline" aria-labelledby="timeline-title">
      {!shown && <div className="viewport-sentinel" aria-hidden="true" ref={watch} />}
      <h2 id="timeline-title">Timeline</h2>
      <div className="tl-box card" id={SLOT} data-slug={slug} style={boxStyle(narrow, wide)}>
        <div className="tl-skeleton">
          <span className="tl-sk-said">Reading the timeline…</span>
          <span className="tl-sk-line tl-sk-head" aria-hidden="true" />
          {Array.from({ length: rows }, (_, row) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: the placeholder rows are alike, and only their count changes
            <span key={row} className="tl-sk-row" aria-hidden="true">
              <span className="tl-sk-line tl-sk-at" />
              <span
                className="tl-sk-line tl-sk-label"
                style={{ width: `${30 + ((row * 17) % 40)}%` }}
              />
              <span
                className="tl-sk-line tl-sk-bar"
                style={{ width: `${20 + ((row * 29) % 50)}%` }}
              />
            </span>
          ))}
        </div>
        {shown && Timeline !== null && (
          // Where the Dioxus shell put the timeline module's root, which the box's style sheet lays
          // out: its presence says the chunk has come.
          <div id="timeline-root">
            <Timeline
              slug={slug}
              thread={thread}
              details={details}
              part={answer}
              tokens={tokens.value}
              read={read}
              onDrawn={(wideDraw, natural) => keep(slug, wideDraw, natural)}
            />
          </div>
        )}
      </div>
      <div className="tl-message-place" ref={setDetails} />
    </section>
  );
}
