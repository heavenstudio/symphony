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
  isRead,
  NO_TOASTS,
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
    const timers = new Set<ReturnType<typeof setTimeout>>();
    const refresh = () => {
      const why = snapshot("reviewFrameError");
      if (typeof why === "string") {
        setTrouble(why);
        return;
      }
      const items = readActivity(snapshot("reviewActivitySnapshot"));
      const cards = readPending(snapshot("reviewPendingSnapshot"));
      if (items === null || cards === null) return;
      const ids = readIds(items.map((item) => item.id)) ?? [];
      if (seen.current === null) {
        seen.current = ids;
        setSeenIds(ids);
        saveSeen(ids);
      }
      const previous = known.current;
      if (previous !== null) {
        const added = items.filter(
          (item) => !previous.includes(item.id) && !isRead(item, seen.current),
        );
        if (added.length > 0) {
          const next = arrive(shownToasts.current, added);
          toast(next);
          const timer = setTimeout(() => {
            timers.delete(timer);
            toast(expire(shownToasts.current, next.batches));
          }, TOAST_MS);
          timers.add(timer);
        }
      }
      known.current = ids;
      setActivity(items);
      setPending(cards);
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
        <aside id="activity-panel" aria-label="Recent activity">
          <div className="panel-heading">
            <h2>Recent activity</h2>
            <Button
              type="button"
              aria-label="Close activity"
              onClick={() => {
                showActivity(false);
                returnFocus("#bell");
              }}
            >
              ×
            </Button>
          </div>
          {trouble !== null && <p role="status">{`Activity could not be refreshed: ${trouble}`}</p>}
          {activity.length === 0 && <p className="muted">No recent activity.</p>}
          <ul>
            {activity.map((item) => (
              <li key={item.id} className={isRead(item, seenIds) ? "" : "unread"}>
                <a
                  href={href(item.href)}
                  onClick={() => {
                    markSeen(item.id);
                    showActivity(false);
                  }}
                >
                  {`${item.kind} · ${item.slug}`}
                </a>
                <div className="meta">{`${item.path} · ${item.ts}`}</div>
                <div className="excerpt">{item.text}</div>
              </li>
            ))}
          </ul>
        </aside>
      )}
      <div id="toasts" aria-live="polite">
        {toasts.shown.map(([, item]) => (
          <div className="pop" key={item.id}>
            <a href={href(item.href)} onClick={() => markSeen(item.id)}>
              {`${item.kind} · ${item.slug}`}
            </a>
            <Button
              type="button"
              aria-label="Dismiss notification"
              onClick={() => toast(dismiss(shownToasts.current, item.id))}
            >
              ×
            </Button>
          </div>
        ))}
      </div>
      <PendingControls route={route} pending={pending} />
    </>
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
    const timer = setInterval(measure, 200);
    return () => clearInterval(timer);
  }, [route, pending]);
  useEffect(() => {
    const keyed = (event: KeyboardEvent) => {
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (event.target instanceof Element && event.target.closest(KEYS_TAKEN)) return;
      const target =
        event.key === "[" ? latest.current[0] : event.key === "]" ? latest.current[1] : null;
      if (target === null) return;
      event.preventDefault();
      goPending(target);
    };
    addEventListener("keydown", keyed);
    return () => removeEventListener("keydown", keyed);
  }, []);
  if (pending.items.length === 0) return null;
  const [prev, next] = targets;
  return (
    <nav className="pending-nav frame-pending" aria-label="Cards waiting on you">
      <Button
        type="button"
        data-pending-action="prev"
        disabled={prev === null}
        title="Previous pending decision ([)"
        onClick={() => prev !== null && goPending(prev)}
      >
        ← prev
      </Button>
      <Button
        type="button"
        data-pending-action="next"
        disabled={next === null}
        title="Next pending decision (])"
        onClick={() => next !== null && goPending(next)}
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
const BOARD_REACH: Reach = { pictures: `main img, ${CHAT_PICTURES}`, views: true };

/** What the panel's media layer answers for on `route`: the chat's pictures on every page, and on
 * the task board and the agents page, which have no media layer of their own, the page's media
 * too. Elsewhere the page's own layer runs the page's mockups and viewers, once. */
export function reachOf(route: Route): Reach {
  return route.path === "tasks" || route.path === "agents" ? BOARD_REACH : CHAT_REACH;
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
