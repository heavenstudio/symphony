// The shared header and first-paint metadata. The closed controls, the conversation and the typed
// activity and pending views live in the persistent panel's module, which the shell loads on a
// click or a toast: the frame reads `/api/activity` and `/api/pending` and leaves them on the
// window for it (`reviewActivitySnapshot`, `reviewPendingSnapshot`), saying `review:frame`.

import {
  createContext,
  Fragment,
  type ReactNode,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { Button } from "./button";
import { useMount } from "./mount";
import { crumbs, headerLinks, href, project, scoped } from "./project";
import { getJson, said } from "./request";
import type { Route } from "./route";
import { field, list, publish, remember, snapshot, useWindowEvents } from "./shared";
import { keepsPanelBaseline, MetadataReads } from "./state";
import { load, save } from "./storage";
import { PAGE_NAMES } from "./wire";

/** What a page's feed tells the frame: read the metadata again, or what moved. */
export interface FrameRefresh {
  refresh(): void;
  /** The stream opened (again): an open chat reads once, and the metadata maybe. */
  connected(): void;
  /** An event the feed heard: the regions that move the frame's metadata or the asks. */
  heard(delta: unknown): void;
}

const FrameContext = createContext<FrameRefresh>({
  refresh() {},
  connected() {},
  heard() {},
});

export function useFrame(): FrameRefresh {
  return useContext(FrameContext);
}

/** How many times the frame was asked to read its metadata again: a page's reads that follow it. */
const RefreshedContext = createContext(0);

/** The frame's count of refreshes (`FrameRefresh.refresh`), which a page reads again on. */
export function useRefreshed(): number {
  return useContext(RefreshedContext);
}

/** Tell every module's ask readers what the page's feed heard of the asks. */
function asksMoved(moved: unknown): void {
  const event = new Event("review:asks") as Event & { moved?: unknown };
  event.moved = moved;
  dispatchEvent(event);
}

/** Ask the panel's module to move focus into `which` once it shows, remembering the opener. */
function focusOnOpen(which: "quick" | "activity"): void {
  remember("reviewPanelOpener", document.activeElement ?? null);
  remember("reviewPanelFocus", which);
}

function quickOpen(): void {
  focusOnOpen("quick");
  let draft = snapshot("reviewQuickDraft");
  if (typeof draft !== "object" || draft === null) {
    try {
      draft = JSON.parse(load("review-quick-draft") ?? "{}");
    } catch {
      draft = {};
    }
    if (typeof draft !== "object" || draft === null) draft = {};
  }
  const open = draft as Record<string, unknown>;
  open.open = true;
  if (typeof open.priority !== "string") open.priority = "2";
  remember("reviewQuickDraft", open);
  remember("reviewActivityOpen", false);
  publish("review:quick");
  publish("review:activity");
}

/** Opens the activity panel, as the header's activity button does while it is closed. */
export function openActivity(): void {
  focusOnOpen("activity");
  // Activity and Appearance share the header's right edge: one open closes the other.
  remember("reviewAppearanceOpen", false);
  publish("review:appearance");
  remember("reviewActivityOpen", true);
  publish("review:activity");
}

/** The activity ids this device has seen, as the panel keeps them; none before its first read. */
function seenIds(): unknown {
  try {
    return JSON.parse(load("docs-review-seen") ?? "null") ?? undefined;
  } catch {
    return undefined;
  }
}

function unread(): boolean {
  const seen = list(seenIds());
  return list(field(snapshot("reviewActivitySnapshot"), "items")).some(
    (item) => field(item, "answered") !== true && !seen.includes(field(item, "id")),
  );
}

const FRAME_EVENTS = [
  "review:frame",
  "review:quick",
  "review:activity",
  "review:panel-ready",
  "review:frame-seen",
  "review:appearance",
] as const;

/**
 * Reads the activity and the pending, and leaves them on the window for the panel, saying
 * `review:frame`, unless `current` says, once they answer, that a newer read or the page's end
 * replaced this one: then nothing is written. Whether to read again (`MetadataReads`).
 */
export async function readMetadata(reads: MetadataReads, current: () => boolean): Promise<boolean> {
  const started = reads.start();
  const [activity, pending] = await Promise.allSettled([
    getJson("/api/activity"),
    getJson("/api/pending"),
  ]);
  if (!current()) return false;
  const retry = reads.finished(
    started,
    activity.status === "fulfilled" && pending.status === "fulfilled",
  );
  if (activity.status === "fulfilled" && pending.status === "fulfilled") {
    const previous = snapshot("reviewKnownIds");
    const ids = list(field(activity.value, "items")).map((item) => field(item, "id"));
    const added = Array.isArray(previous) && ids.some((id) => !previous.includes(id));
    if (!Array.isArray(seenIds())) save("docs-review-seen", JSON.stringify(ids));
    const listed = (value: unknown) => (Array.isArray(value) ? value : null);
    if (
      !keepsPanelBaseline(
        listed(snapshot("reviewPreviousIds")),
        listed(previous),
        snapshot("reviewPanelReady") === true,
      )
    ) {
      remember("reviewPreviousIds", previous);
    }
    remember("reviewKnownIds", ids);
    remember("reviewActivitySnapshot", activity.value);
    remember("reviewPendingSnapshot", pending.value);
    remember("reviewFrameError", null);
    publish("review:frame");
    if (added) publish("review:activity-arrived");
  } else {
    const failure = activity.status === "rejected" ? activity : pending;
    remember("reviewFrameError", said((failure as PromiseRejectedResult).reason));
    publish("review:frame");
  }
  return retry;
}

/**
 * The activity and pending reads, and what they leave on the window for the panel; with the count
 * of the refreshes asked.
 */
function useMetadata(): [FrameRefresh, number] {
  const [asked, setAsked] = useState(0);
  const reads = useRef(new MetadataReads());
  const generation = useRef(0);
  const mount = useMount();
  const [frame] = useState<FrameRefresh>(() => {
    const refresh = () => setAsked((count) => count + 1);
    return {
      refresh,
      connected() {
        // A stream that opens again may have missed an answer: every open chat reads once.
        asksMoved(true);
        if (reads.current.connected()) refresh();
      },
      heard(delta) {
        const changes = field(delta, "changes");
        if (!Array.isArray(changes)) return;
        let moved = false;
        for (const change of changes) {
          const region = field(change, "region");
          if (region === "activity" || region === "pending" || region === "docs") moved = true;
          else if (region === "asks") asksMoved(field(change, "data"));
        }
        if (moved) refresh();
      },
    };
  });
  useEffect(() => {
    void asked;
    generation.current += 1;
    const ticket = generation.current;
    const current = () => ticket === generation.current && !mount.aborted;
    void readMetadata(reads.current, current).then((retry) => {
      if (retry) frame.refresh();
    });
  }, [asked, frame, mount]);
  return [frame, asked];
}

function AppearanceIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M12 3a9 9 0 0 1 0 18z" fill="currentColor" />
    </svg>
  );
}

function BellIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
      <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
    </svg>
  );
}

/** The Symphony's own pages, which stay the global view's on a branch's document. */
const GLOBAL = ["/", "/queue", "/tasks", "/reports", "/agents"];

export interface FrameProps {
  route: Route;
  children: ReactNode;
}

/** The page's frame: the header, the page, the floating controls' dock, and the `+` button. */
export function Frame({ route, children }: FrameProps) {
  const [frame, refreshed] = useMetadata();
  useWindowEvents(FRAME_EVENTS);
  const open = field(snapshot("reviewQuickDraft"), "open") === true;
  const panel = snapshot("reviewActivityOpen") === true;
  const appearance = snapshot("reviewAppearanceOpen") === true;
  const hasPending = list(field(snapshot("reviewPendingSnapshot"), "items")).length > 0;
  const view = project();
  const links = headerLinks(view, route.path, route.document);
  const trail = crumbs(view, route.path);
  return (
    <FrameContext.Provider value={frame}>
      <RefreshedContext.Provider value={refreshed}>
        <div className={open ? "app-frame quick-expanded" : "app-frame"}>
          <header className="app-header" id="top">
            {(view.base ?? "") !== "" && (
              <a href="/" data-native="true" className="project-switch">
                {PAGE_NAMES.PROJECTS}
              </a>
            )}
            <nav className="app-pages" aria-label="Pages">
              {links.map((link) => (
                <a
                  key={link.href}
                  href={href(
                    GLOBAL.includes(link.href) ? link.href : scoped(route.scope, link.href),
                  )}
                  className={link.active ? "active" : ""}
                  aria-current={link.active ? "page" : undefined}
                >
                  {link.label}
                </a>
              ))}
            </nav>
            {trail.length > 0 && (
              <nav className="crumbs" aria-label="Breadcrumbs">
                {trail.map((crumb, at) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: a crumb is its place in the trail
                  <Fragment key={at}>
                    {at > 0 && " / "}
                    {at + 1 < trail.length ? (
                      <a href={href(scoped(route.scope, `/${trail.slice(0, at + 1).join("/")}/`))}>
                        {crumb}
                      </a>
                    ) : (
                      <span>{crumb}</span>
                    )}
                  </Fragment>
                ))}
              </nav>
            )}
            {/* Appearance is a setting of this project on this device: its panel is the persistent
              module's, which this button loads, as the bell loads activity's. */}
            <Button
              id="appearance"
              type="button"
              aria-label="Appearance"
              title="Appearance"
              aria-expanded={appearance}
              aria-controls="appearance-panel"
              onClick={() => {
                remember("reviewAppearanceOpen", !appearance);
                remember("reviewActivityOpen", false);
                publish("review:activity");
                publish("review:appearance");
              }}
            >
              <AppearanceIcon />
            </Button>
            <Button
              id="bell"
              type="button"
              aria-expanded={panel}
              aria-controls="activity-panel"
              onClick={() => {
                if (panel) {
                  remember("reviewActivityOpen", false);
                  publish("review:activity");
                } else {
                  openActivity();
                }
              }}
            >
              <BellIcon />
              {" Activity"}
              {/* An image role names the dot: a plain span's label is no name a reader gives. */}
              {unread() && <span className="dot ok" role="img" aria-label="Unread activity" />}
            </Button>
          </header>
          <div className="frame-page">{children}</div>
          {/* The bar the floating controls sit in (frame.css): the page scrolls clear of it. */}
          <div className="frame-dock" aria-hidden="true" />
          {hasPending && snapshot("reviewPanelReady") !== true && (
            <nav className="pending-nav frame-pending" aria-label="Cards waiting on you">
              <Button type="button" data-pending-step="prev" title="Previous pending decision ([)">
                ← prev
              </Button>
              <Button type="button" data-pending-step="next" title="Next pending decision (])">
                next →
              </Button>
              <a href="#top">↑ top</a>
            </nav>
          )}
          <Button
            id="quick-open"
            type="button"
            aria-label="Chat or add a task"
            aria-controls="quick-sheet"
            aria-expanded={open}
            onClick={quickOpen}
          >
            +
          </Button>
        </div>
      </RefreshedContext.Provider>
    </FrameContext.Provider>
  );
}
