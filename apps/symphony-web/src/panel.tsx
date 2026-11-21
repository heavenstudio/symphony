// The panel, the shell's persistent kind `panel` (src/kinds/panel.tsx), mounted in `#quick-root`
// beside every page and kept while the pages change, so the conversation, the task form and their
// places stay: the `+` sheet (quick.tsx), the activity list and its toasts, the pending bar, the
// header's Appearance panel (appearance.tsx), and the media layer of the chat's pictures. It has no
// feed of its own: the frame (frame.tsx) reads the activity and the pending cards and leaves them on
// the window (`review:frame`), and tells it what the page's feed heard of the asks.

import { useCallback, useEffect, useRef, useState } from "react";
import { AppearancePanel } from "./appearance";
import { Button } from "./button";
import { MediaLayer, type Reach } from "./media-layer";
import { returnFocus, takeFocus, useEscape } from "./panel-focus";
import {
  arrive,
  type Draft,
  dismiss,
  expire,
  frameNews,
  isRead,
  NO_TOASTS,
  pendingStep,
  pendingTargets,
  readActivity,
  readIds,
  readPending,
  TOAST_MS,
  type Toasts,
} from "./panel-model";
import { href } from "./project";
import { type ChangeDraft, QuickPanel, useDraft } from "./quick";
import { type Route, useRoute } from "./route";
import { publish, remember, snapshot } from "./shared";
import { load, save } from "./storage";
import type { ActivityItem, Pending, PendingItem } from "./wire";

/** The activity list's first control, where the focus goes when it opens. */
const ACTIVITY_FIRST = ["#activity-panel :is(a[href], button)"];

/** The activity ids this device has seen, as it kept them; none before the first read. */
function loadSeen(): string[] | null {
  try {
    return readIds(JSON.parse(load("docs-review-seen") ?? "null"));
  } catch {
    return null;
  }
}

function saveSeen(seen: readonly string[]): void {
  save("docs-review-seen", JSON.stringify(seen));
}

interface ExtrasProps {
  route: Route;
  draft: Draft;
  change: ChangeDraft;
}

/** The activity list, its toasts and the pending bar, as the frame's reads leave them. */
function FrameExtras({ route, draft, change }: ExtrasProps) {
  // Drawn from the frame's last read at once, so the panel's first frame shows what it holds.
  const [activity, setActivity] = useState<ActivityItem[]>(
    () => readActivity(snapshot("reviewActivitySnapshot")) ?? [],
  );
  const [pending, setPending] = useState<Pending>(
    () => readPending(snapshot("reviewPendingSnapshot")) ?? { items: [], docs: [] },
  );
  const [seenIds, setSeenIds] = useState(loadSeen);
  const seen = useRef(seenIds);
  seen.current = seenIds;
  const known = useRef(readIds(snapshot("reviewPreviousIds")));
  const [toasts, setToasts] = useState<Toasts>(NO_TOASTS);
  const shownToasts = useRef(toasts);
  const [panel, setPanel] = useState(() => snapshot("reviewActivityOpen") === true);
  const shownPanel = useRef(panel);
  const [trouble, setTrouble] = useState<string | null>(() => {
    const why = snapshot("reviewFrameError");
    return typeof why === "string" ? why : null;
  });
  const latestDraft = useRef(draft);
  latestDraft.current = draft;

  const showActivity = (open: boolean) => {
    shownPanel.current = open;
    setPanel(open);
    remember("reviewActivityOpen", open);
    publish("review:activity");
  };
  /** The item `id` seen: kept, and said to the frame, whose unread dot follows it. */
  const markSeen = (id: string) => {
    const next = readIds([...(seen.current ?? []), id]) ?? [id];
    seen.current = next;
    setSeenIds(next);
    saveSeen(next);
    publish("review:frame-seen");
  };
  const toast = useCallback((next: Toasts) => {
    shownToasts.current = next;
    setToasts(next);
  }, []);

  // The list opened or closed from the header: opened, it takes the focus and closes the sheet.
  useEffect(() => {
    const heard = () => {
      const open = snapshot("reviewActivityOpen") === true;
      if (open !== shownPanel.current) {
        shownPanel.current = open;
        setPanel(open);
        if (open) takeFocus("activity", ACTIVITY_FIRST);
      }
      if (open && latestDraft.current.open) change(() => ({ open: false }));
    };
    addEventListener("review:activity", heard);
    return () => removeEventListener("review:activity", heard);
  }, [change]);
  useEffect(() => {
    if (snapshot("reviewActivityOpen") === true) takeFocus("activity", ACTIVITY_FIRST);
  }, []);
  useEscape("#activity-panel", () => {
    showActivity(false);
    returnFocus("#bell");
  });

  // What the frame read: the items that arrived since its last read and are unread come up as
  // toasts, a batch at a time.
  useEffect(() => {
    // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: the toasts' timers, below
    const timers = new Set<ReturnType<typeof setTimeout>>();
    const refresh = () => {
      const news = frameNews(
        snapshot("reviewFrameError"),
        snapshot("reviewActivitySnapshot"),
        snapshot("reviewPendingSnapshot"),
        known.current,
        seen.current,
      );
      if (news === null) return;
      if ("trouble" in news) {
        setTrouble(news.trouble);
        return;
      }
      if (news.first) {
        seen.current = news.seen;
        setSeenIds(news.seen);
        saveSeen(news.seen);
      }
      if (news.added.length > 0) {
        const next = arrive(shownToasts.current, news.added);
        toast(next);
        // biome-ignore lint/style/noRestrictedGlobals: no-timed-waits baseline: a batch of toasts leaves TOAST_MS after it came, a change shown by a length of time
        const timer = setTimeout(() => {
          timers.delete(timer);
          toast(expire(shownToasts.current, next.batches));
        }, TOAST_MS);
        timers.add(timer);
      }
      known.current = news.ids;
      setActivity(news.items);
      setPending(news.cards);
      setTrouble(null);
    };
    refresh();
    addEventListener("review:frame", refresh);
    return () => {
      removeEventListener("review:frame", refresh);
      for (const timer of timers) clearTimeout(timer);
    };
  }, [toast]);

  return (
    <>
      {panel && (
        <ActivityList
          activity={activity}
          seen={seenIds}
          trouble={trouble}
          close={() => {
            showActivity(false);
            returnFocus("#bell");
          }}
          opened={(id) => {
            markSeen(id);
            showActivity(false);
          }}
        />
      )}
      <ToastList
        toasts={toasts}
        opened={markSeen}
        dismissed={(id) => toast(dismiss(shownToasts.current, id))}
      />
      <PendingControls route={route} pending={pending} />
    </>
  );
}

export interface ActivityListProps {
  activity: readonly ActivityItem[];
  /** The ids this device has seen; none before its first read. */
  seen: readonly string[] | null;
  /** Why the frame's last read failed, or none. */
  trouble: string | null;
  close: () => void;
  /** An item's link followed, by its id. */
  opened: (id: string) => void;
}

/** The open activity list (`#activity-panel`): each item, the unread marked, and its trouble. */
export function ActivityList({ activity, seen, trouble, close, opened }: ActivityListProps) {
  return (
    <aside id="activity-panel" aria-label="Recent activity">
      <div className="panel-heading">
        <h2>Recent activity</h2>
        <Button type="button" aria-label="Close activity" onClick={close}>
          ×
        </Button>
      </div>
      {trouble !== null && <p role="status">{`Activity could not be refreshed: ${trouble}`}</p>}
      {activity.length === 0 && <p className="muted">No recent activity.</p>}
      <ul>
        {activity.map((item) => (
          <li key={item.id} className={isRead(item, seen) ? "" : "unread"}>
            <a href={href(item.href)} onClick={() => opened(item.id)}>
              {`${item.kind} · ${item.slug}`}
            </a>
            <div className="meta">{`${item.path} · ${item.ts}`}</div>
            <div className="excerpt">{item.text}</div>
          </li>
        ))}
      </ul>
    </aside>
  );
}

export interface ToastListProps {
  toasts: Toasts;
  /** A toast's link followed, by its item's id. */
  opened: (id: string) => void;
  /** A toast's × pressed, by its item's id. */
  dismissed: (id: string) => void;
}

/** The toasts up (`#toasts`), each a link to its item and its dismiss button. */
export function ToastList({ toasts, opened, dismissed }: ToastListProps) {
  return (
    <div id="toasts" aria-live="polite">
      {toasts.shown.map(([, item]) => (
        <div className="pop" key={item.id}>
          <a href={href(item.href)} onClick={() => opened(item.id)}>
            {`${item.kind} · ${item.slug}`}
          </a>
          <Button
            type="button"
            aria-label="Dismiss notification"
            onClick={() => dismissed(item.id)}
          >
            ×
          </Button>
        </div>
      ))}
    </div>
  );
}

/** The fields and places a pending step's key may not be taken from. */
const KEYS_TAKEN =
  "input, textarea, select, [contenteditable], dialog[open],[role=dialog],[role=listbox],[role=menu]";

/** The pending cards the bar's steps go to from where the reader is on `route` now. */
function currentTargets(route: Route, pending: Pending): [PendingItem | null, PendingItem | null] {
  const path = route.scope === "" ? route.path : `@${route.scope}/${route.path}`;
  let anchor: string;
  try {
    anchor = decodeURIComponent(location.hash.replace(/^#+/, ""));
  } catch {
    anchor = "";
  }
  const top = (slug: string) => document.getElementById(slug)?.getBoundingClientRect().top ?? null;
  return pendingTargets(path, pending, top, anchor, innerHeight);
}

/** Go to a pending card by a click on a link to it, which the shell follows in place. */
function goPending(item: PendingItem): void {
  const link = document.createElement("a");
  link.setAttribute("href", href(item.href));
  document.body.append(link);
  link.click();
  link.remove();
}

const same = (a: PendingItem | null, b: PendingItem | null) =>
  JSON.stringify(a) === JSON.stringify(b);

/** The bar of the cards waiting on the reader: the one before and the one after where they are,
 * measured as they scroll, and `[` and `]` for the same steps. */
function PendingControls({ route, pending }: { route: Route; pending: Pending }) {
  const [targets, setTargets] = useState(() => currentTargets(route, pending));
  const latest = useRef(targets);
  useEffect(() => {
    const measure = () => {
      const next = currentTargets(route, pending);
      if (same(next[0], latest.current[0]) && same(next[1], latest.current[1])) return;
      latest.current = next;
      setTargets(next);
    };
    measure();
    // Measured again at each change that can move the cards or the reader's place among them: a
    // scroll of the page or a box in it, the window resized, the anchor followed, the page's
    // height changed as it draws, or a card drawn in where its ghost stood.
    const watching = new AbortController();
    const options = { capture: true, passive: true, signal: watching.signal };
    addEventListener("scroll", measure, options);
    addEventListener("resize", measure, { signal: watching.signal });
    addEventListener("hashchange", measure, { signal: watching.signal });
    const grown = new ResizeObserver(measure);
    grown.observe(document.documentElement);
    const drawn = new MutationObserver(measure);
    drawn.observe(document.body, { childList: true, subtree: true });
    return () => {
      watching.abort();
      grown.disconnect();
      drawn.disconnect();
    };
  }, [route, pending]);
  useEffect(() => {
    const keyed = (event: KeyboardEvent) => {
      const typing = event.target instanceof Element && event.target.closest(KEYS_TAKEN) !== null;
      const target = pendingStep(event, typing, latest.current);
      if (target === null) return;
      event.preventDefault();
      goPending(target);
    };
    addEventListener("keydown", keyed);
    return () => removeEventListener("keydown", keyed);
  }, []);
  if (pending.items.length === 0) return null;
  return <PendingBar targets={targets} go={goPending} />;
}

export interface PendingBarProps {
  /** The cards `← prev` and `next →` go to, none where there is none. */
  targets: readonly [PendingItem | null, PendingItem | null];
  go: (item: PendingItem) => void;
}

/** The pending bar's steps and `↑ top`, a step with nowhere to go disabled. */
export function PendingBar({ targets: [prev, next], go }: PendingBarProps) {
  return (
    <nav className="pending-nav frame-pending" aria-label="Cards waiting on you">
      <Button
        type="button"
        data-pending-action="prev"
        disabled={prev === null}
        title="Previous pending decision ([)"
        onClick={() => prev !== null && go(prev)}
      >
        ← prev
      </Button>
      <Button
        type="button"
        data-pending-action="next"
        disabled={next === null}
        title="Next pending decision (])"
        onClick={() => next !== null && go(next)}
      >
        next →
      </Button>
      <a href="#top">↑ top</a>
    </nav>
  );
}

/** The pictures of the chat's messages, which open the viewer as a set of their own. */
const CHAT_PICTURES = "#quick-sheet .quick-message img";
const CHAT_REACH: Reach = { pictures: CHAT_PICTURES, views: false };
const WHOLE_REACH: Reach = { pictures: `main img, ${CHAT_PICTURES}`, views: true };

/** Whether `route`'s kind mounts a media layer of its own: home, a document and a task's page. */
function ownLayer(route: Route): boolean {
  return route.path === "" || route.document || route.path.startsWith("task/");
}

/** What the panel's media layer answers for on `route`: the chat's pictures on every page, and on
 * a page with no media layer of its own (the task board, the agents page, the queue, Reports,
 * Runs and Settings) the page's media and the chat's mockups and viewers too. Elsewhere the page's own layer
 * runs every mockup and viewer on it, the chat's among them, once. */
export function reachOf(route: Route): Reach {
  return ownLayer(route) ? CHAT_REACH : WHOLE_REACH;
}

export function PanelApp() {
  const route = useRoute();
  const [draft, change] = useDraft();
  const reach = reachOf(route);
  return (
    <div className="app-frame">
      <FrameExtras route={route} draft={draft} change={change} />
      <AppearancePanel />
      <MediaLayer owner="panel-pictures" reach={reach} />
      <QuickPanel route={route} draft={draft} change={change} />
    </div>
  );
}
