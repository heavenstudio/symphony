// One document, `/docs/X.md` (`docs_live.rs`, `docs.rs`, `doc_components.rs`). The outline first
// (`/api/document?view=outline`: the head, the table of contents and every section's key, place and
// anchors), every section a `Near` placeholder of its reserved height; a section that nears the
// screen asks for its body, and the bodies and card threads asked for within 50 ms are read
// together (`/api/section?format=blocks&keys=JSON_LIST`), installed once the feed's cursor has
// reached the read's version. The document's feed (`/api/feed?since=V&document=PATH&sections=
// JSON_LIST`) opens at the outline's version and again, at its cursor, when the requested keys
// change; its document changes are applied as `docs_live.rs` applies them (a requested section
// that comes without its body keeps the body it had under the same hash, or is read again). A lost
// stream (an error, 12.5 s of silence, an event the page cannot read) marks the page stale and
// reads the outline again after 1 s doubling to 30 s. The body waits under its ghost until the
// sections on the first screen are in (`Settle`, 3 s at most); `document-ready` is marked once
// every requested section is in, as `docs_live.rs` marks it.
//
// Each section has its editor, as the Dioxus page draws it: a card's verdict and message form
// (Approve, Reject, Question, Later, Send message, Dismiss, Reset to pending, Discard draft), and
// a prose section's comment form. A verdict posts `/api/decision` (path, text, slug, state, note,
// tap), a message `/api/message`, a comment `/api/comment` (path, text, after_line, kind), Later
// `/api/postpone`; the answer closes the form and says what was saved, and the card releases so
// the owner's own answer shows at once while the feed brings it. A card's link to a section opens
// its excerpt beside it.
//
// Left out (named in the comparison): asks (questions on a document: their section's heading
// is drawn, nothing is read); attachments (the controls are drawn, with the style the Dioxus attach
// module puts in the page, and take no file); versions, history and diffs (the closed Versions
// summary is drawn and reads nothing); drafts and verdict taps kept on the device (a tap is new
// for every press, so a save the server may have written is not retried under its tap, and an
// uncertain failure is a plain error); the device cache; the comment kind's select popup (the
// kind stays `comment`); folders and a folder's intro; a worktree's `@scope`; the frame's refresh
// on feed events and the server notice; the removed document's notice (the page keeps showing
// it); ghosts beyond the page's and a section's skeleton (`GhostDocument`, `section-loading`).
// The hold rules are kept as far as the verdict needs them: a section with an open form, a
// focused field or a draft keeps its markup and place until free (checked every 400 ms), a
// section still reading keeps the body it had, and a card's own saved answer releases it until
// it shows one more line of the owner's.
import { createContext, memo, useContext, useEffect, useMemo, useRef, useState } from "react";
import { encode, get, getFound, post, sleep } from "../api.js";
import { Blocks, CardAnchorContext, ExcerptContext, HistoricalBlocks, Spans } from "../blocks.jsx";
import { Button, SelectTrigger, Textarea } from "../controls.jsx";
import { mark, PageNotices } from "../frame.jsx";
import { Near, Skeleton } from "../near.jsx";
import { documentsHref, href } from "../project.js";
import ATTACH_STYLE from "../../../../apps/symphony-ui/assets/attach.css?inline";

const TOC_MIN_HEADINGS = 3;
const RECONNECT = "Reconnecting: this document may be out of date";
const CHECKING = "Checking…";
const SETTLE_MS = 3000;
const SILENCE_MS = 12500;
const FIRST_WAIT = 1000;
const LAST_WAIT = 30000;
const LAST = Number.MAX_SAFE_INTEGER;

// ---- docs.rs: document snapshots and keyed feed changes ----

const settledGroup = (card) => card?.group === "resolved" || card?.group === "rejected";

/** `DocumentChange` applied to the latest copy; unchanged sections keep their objects. */
function change(state, delta) {
  if (!delta || delta.path !== state.latest.path) return state;
  if (delta.removed) return { ...state, removed: true };
  const doc = state.latest;
  let sections = doc.sections;
  if (delta.deleted?.length) sections = sections.filter((section) => !delta.deleted.includes(section.key));
  if (delta.sections?.length) {
    sections = sections.slice();
    for (const section of delta.sections) {
      const at = sections.findIndex((old) => old.key === section.key);
      if (at >= 0) sections[at] = section;
      else sections.push(section);
    }
  }
  if (delta.positions?.length) {
    sections = sections === doc.sections ? sections.slice() : sections;
    for (const place of delta.positions) {
      const at = sections.findIndex((old) => old.key === place.key);
      const old = sections[at];
      if (old && (old.start !== place.start || old.end !== place.end || (old.part ?? null) !== (place.part ?? null))) {
        sections[at] = { ...old, start: place.start, end: place.end, part: place.part ?? null };
      }
    }
  }
  if (delta.order) {
    const rank = new Map(delta.order.map((key, index) => [key, index]));
    sections = sections.slice().sort((a, b) => (rank.get(a.key) ?? LAST) - (rank.get(b.key) ?? LAST));
  }
  let threads = doc.threads;
  for (const [slug, fragment] of Object.entries(delta.threads ?? {})) {
    threads = threads === doc.threads ? { ...threads } : threads;
    if (fragment) threads[slug] = fragment;
    else delete threads[slug];
  }
  let cards = doc.cards;
  for (const card of delta.cards ?? []) {
    cards = cards === doc.cards ? cards.slice() : cards;
    if (card.removed) cards = cards.filter((old) => old.slug !== card.key);
    if (card.value) {
      const at = cards.findIndex((old) => old.slug === card.key);
      if (at >= 0) cards[at] = card.value;
      else cards.push(card.value);
    }
  }
  const latest = { ...doc, sections, threads, cards };
  for (const name of ["title", "waiting", "crumbs", "headings", "tasks"]) if (delta[name] != null) latest[name] = delta[name];
  if (delta.comparison != null) latest.comparison = delta.comparison[0] ?? null;
  return { ...state, latest };
}

/** Whether two copies hold the same parts, part for part. */
function sameDocument(a, b) {
  if (a === b) return true;
  for (const name of ["version", "path", "title", "waiting", "crumbs", "headings", "tasks", "comparison"]) if (a[name] !== b[name]) return false;
  const same = (x, y) => x.length === y.length && x.every((item, index) => item === y[index]);
  if (!same(a.sections, b.sections) || !same(a.cards, b.cards)) return false;
  const keys = Object.keys(a.threads);
  return keys.length === Object.keys(b.threads).length && keys.every((key) => a.threads[key] === b.threads[key]);
}

/** `DocumentState::refresh`: show the latest copy but the `held` sections, kept as and where shown. */
function refresh(state, held) {
  const { latest, shown } = state;
  if (held.size === 0) return shown === latest ? state : { ...state, shown: latest };
  const sections = latest.sections.slice();
  const threads = { ...latest.threads };
  const cards = latest.cards.slice();
  for (const old of shown.sections) {
    if (!held.has(old.key)) continue;
    const at = sections.findIndex((section) => section.key === old.key);
    if (at >= 0) {
      const now = sections[at];
      sections[at] = now.start === old.start && now.end === old.end && now.part === old.part ? old : { ...old, start: now.start, end: now.end, part: now.part };
    } else sections.push(old);
    if (old.key.startsWith("card:")) {
      const slug = old.key.slice("card:".length);
      if (shown.threads[slug]) threads[slug] = shown.threads[slug];
      else delete threads[slug];
      const card = shown.cards.find((c) => c.slug === slug);
      if (card) {
        const target = cards.findIndex((c) => c.slug === slug);
        if (target >= 0) cards[target] = card;
        else cards.push(card);
      }
    }
  }
  // Do not move a busy block under the owner's pointer when the register regroups.
  const place = new Map(shown.sections.map((section, index) => [section.key, index]));
  sections.sort((a, b) => (place.get(a.key) ?? LAST) - (place.get(b.key) ?? LAST));
  const next = { ...latest, sections, threads, cards };
  if (sameDocument(next, latest)) return { ...state, shown: latest };
  if (sameDocument(next, shown)) return state;
  return { ...state, shown: next };
}

/** `docs::held`: the engaged sections but a released card's, a closed draft's once its section is gone, and those still reading. */
function heldKeys(engaged, drafts, released, loading, latest) {
  const held = new Set([...engaged].filter((key) => !released.has(key)));
  for (const key of drafts) if (!latest.sections.some((section) => section.key === key)) held.add(key);
  for (const key of loading) held.add(key);
  return held;
}

/** How many lines of the owner's the card `key` (`card:SLUG`) shows in `document`'s thread. */
function ownerLines(document, key) {
  const thread = key.startsWith("card:") ? document.threads[key.slice("card:".length)] : null;
  if (!thread) return 0;
  return thread.blocks.reduce((lines, block) => lines + (block.type === "thread" ? block.entries.filter((entry) => entry.who === "owner").length : 0), 0);
}

/** `docs::settle`: a release ends once its card shows the owner's line, or is gone. */
function settle(released, shown) {
  for (const [key, lines] of released) {
    if (!shown.sections.some((section) => section.key === key) || ownerLines(shown, key) >= lines) released.delete(key);
  }
}

/** The sections the owner is engaged in (a focused field, an open form or details) and those holding only a closed draft. */
function busy() {
  const engaged = new Set();
  const drafts = new Set();
  const keyOf = (node) => node.closest("[data-doc-key]")?.getAttribute("data-doc-key");
  const active = document.activeElement;
  if (active?.matches?.("input, textarea, select, button[aria-haspopup=listbox], [role=option]")) {
    const key = keyOf(active);
    if (key) engaged.add(key);
  }
  for (const node of document.querySelectorAll("[data-busy=true]:not([data-draft=true]), .section-prose details[open], .history details[open]")) {
    const key = keyOf(node);
    if (key) engaged.add(key);
  }
  for (const node of document.querySelectorAll("[data-draft=true]")) {
    const key = keyOf(node);
    if (key) drafts.add(key);
  }
  return { engaged, drafts };
}

/** `keys_query`: the keys as a JSON list, in the order a `BTreeSet` keeps them. */
const keysQuery = (keys) => encode(JSON.stringify([...keys].sort()));

const hashTarget = () => {
  try {
    return decodeURIComponent(location.hash.slice(1));
  } catch {
    return "";
  }
};

/** `navigation::scroll_anchor`: the element the address names, its folds opened, in view. */
function scrollAnchor() {
  const anchor = hashTarget();
  const element = anchor ? document.getElementById(anchor) : null;
  if (!element) return;
  for (let node = element; node; node = node.parentElement) if (node.tagName === "DETAILS") node.open = true;
  element.scrollIntoView();
}

// ---- docs_live.rs: the page's reads and feed ----

const INITIAL = { doc: null, stale: true, trouble: null, missing: false, wanted: new Set(), loaded: new Set(), anchors: {}, postponed: [] };

/**
 * The document at `path` kept live: its outline, its sections' bodies as they are asked for and
 * its feed, each a loop of its own as the Dioxus page spawns them. `render(page)` takes each new
 * state of the page. Returns the page's handle: `want(key)`, `release(key, before)`, `latest()`,
 * `retry()`, `postponed()` and `stop()`.
 */
function liveDocument(path, render) {
  let page = INITIAL;
  let stopped = false;
  let epoch = 0;
  let cursor = 0;
  let away = false;
  let woken = null;
  let ticket = 0;
  /** The cards the owner just saved in, by the lines of the owner's each must show. */
  const released = new Map();
  const update = (changes) => {
    if (stopped) return;
    page = { ...page, ...changes };
    render(page);
  };
  const wake = () => {
    const resolve = woken;
    woken = null;
    resolve?.();
  };
  /** Wait `ms`, or until the reader presses Retry or the page comes back. */
  const pause = (ms) =>
    new Promise((resolve) => {
      const timer = setTimeout(() => {
        woken = null;
        resolve();
      }, ms);
      woken = () => {
        clearTimeout(timer);
        resolve();
      };
    });
  const back = async () => {
    while (away && !stopped) await pause(LAST_WAIT);
  };
  const leave = () => {
    away = true;
  };
  const returned = () => {
    if (away) {
      away = false;
      wake();
    }
  };
  window.addEventListener("pagehide", leave);
  document.addEventListener("freeze", leave);
  window.addEventListener("pageshow", returned);
  document.addEventListener("resume", returned);

  const loading = () => [...page.wanted].filter((key) => !page.loaded.has(key));
  /** Show all but what holds the shown page now; a card's own save that now shows ends its release. */
  const show = (state) => {
    const { engaged, drafts } = busy();
    const next = refresh(state, heldKeys(engaged, drafts, released, loading(), state.latest));
    settle(released, next.shown);
    return next;
  };

  async function readPostponed() {
    const mine = ++ticket;
    try {
      const cards = await get("/api/postponed");
      if (mine === ticket) update({ postponed: cards });
    } catch {}
  }

  /** One feed event; false when the page cannot read it. */
  function delta(data) {
    let value;
    try {
      value = JSON.parse(data);
    } catch {
      return false;
    }
    const version = value?.version;
    if (!Number.isSafeInteger(version) || version < 0) return false;
    cursor = version;
    if (!Array.isArray(value.changes)) return false;
    let state = page.doc;
    let loaded = page.loaded;
    let anchors = page.anchors;
    let order = false;
    for (const one of value.changes) {
      if (one?.region === "queue-item" || one?.region === "queue-item-order") order = true;
      if (one?.region !== "document") continue;
      const data = one.data;
      if (!data || typeof data.path !== "string") return false;
      if (data.path !== path) continue;
      if (data.anchors && typeof data.anchors === "object") anchors = { ...anchors, ...data.anchors };
      const sections = (data.sections ?? []).map((section) => {
        if ((section.blocks ?? []).length || !page.wanted.has(section.key)) return section;
        // A requested section that comes without its body keeps the one it had under its hash.
        const before = state?.latest.sections.find((old) => old.key === section.key);
        if (before && before.hash === section.hash && before.blocks?.length) {
          const placed = before.start === section.start && before.end === section.end && before.part === section.part;
          return placed ? before : { ...section, blocks: before.blocks, comparison: before.comparison };
        }
        if (loaded.has(section.key)) {
          loaded = new Set(loaded);
          loaded.delete(section.key);
        }
        return section;
      });
      if (state) {
        state = change(state, { ...data, sections });
        state = { ...state, latest: { ...state.latest, version } };
      }
    }
    if (state !== page.doc || loaded !== page.loaded || anchors !== page.anchors) {
      page = { ...page, loaded, anchors };
      update({ doc: state ? show(state) : state });
    }
    if (order) readPostponed();
    return true;
  }

  /** Follow the document's feed from the cursor until the stream is lost or the page goes away. */
  const follow = () =>
    new Promise((resolve) => {
      let source = null;
      let subscribed = null;
      let opened = 0;
      let heard = Date.now();
      let timer = null;
      const lost = () => ({ lost: opened ? Date.now() - opened : 0 });
      const finish = (ended) => {
        clearInterval(timer);
        window.removeEventListener("pagehide", gone);
        document.removeEventListener("freeze", gone);
        source?.close();
        source = null;
        resolve(ended);
      };
      const gone = () => finish({ away: true });
      const open = () => {
        source?.close();
        subscribed = page.wanted;
        heard = Date.now();
        const stream = new EventSource(href(`/api/feed?since=${cursor}&document=${encode(path)}&sections=${keysQuery(subscribed)}`));
        source = stream;
        stream.addEventListener("open", () => {
          heard = Date.now();
          if (!opened) opened = heard;
          update({ stale: false, trouble: null });
        });
        stream.addEventListener("ping", () => (heard = Date.now()));
        stream.addEventListener("server", () => (heard = Date.now()));
        stream.addEventListener("delta", (event) => {
          heard = Date.now();
          if (stream === source && !delta(event.data)) finish(lost());
        });
        stream.addEventListener("error", () => stream === source && finish(lost()));
      };
      window.addEventListener("pagehide", gone);
      document.addEventListener("freeze", gone);
      timer = setInterval(() => {
        if (stopped) return finish({ away: true });
        // Changing the requested keys reopens the stream at its current cursor.
        if (page.wanted !== subscribed) return open();
        if (Date.now() - heard >= SILENCE_MS) finish(lost());
      }, 200);
      open();
    });

  async function live() {
    let next = FIRST_WAIT;
    while (!stopped) {
      await back();
      if (stopped) return;
      let ended = { lost: 0 };
      try {
        const outline = await getFound(`/api/document?path=${encode(path)}&view=outline`);
        if (stopped) return;
        if (!outline) update({ missing: true, trouble: null });
        else {
          epoch++;
          cursor = outline.document.version;
          // An outline holds no bodies: the sections asked for keep theirs until read again.
          page = { ...page, missing: false, anchors: outline.anchors, loaded: new Set() };
          const state = { ...(page.doc ?? { shown: outline.document }), latest: outline.document, removed: false };
          update({ doc: show(state) });
          ended = await follow();
        }
      } catch (error) {
        if (stopped) return;
        update({ trouble: error.message });
      }
      if (stopped) return;
      update({ stale: true });
      let wait;
      if (ended.away) {
        next = FIRST_WAIT;
        wait = 0;
      } else {
        if (ended.lost >= LAST_WAIT) next = FIRST_WAIT;
        wait = next;
        next = Math.min(next * 2, LAST_WAIT);
      }
      await pause(wait);
    }
  }

  async function loadSections() {
    while (!stopped) {
      await sleep(50);
      if (stopped) return;
      if (!page.doc) continue;
      const missing = loading();
      if (!missing.length) continue;
      const mine = epoch;
      try {
        const parts = await get(`/api/section?path=${encode(path)}&format=blocks&keys=${keysQuery(missing)}`);
        if (stopped) return;
        if (epoch !== mine) continue;
        // The ordered feed catches up to the read's version first: an older body never
        // overwrites a later change the feed already applied.
        const deadline = Date.now() + 2000;
        while (cursor < parts.version && Date.now() < deadline && epoch === mine && !stopped) await sleep(50);
        if (stopped) return;
        if (cursor < parts.version || epoch !== mine) continue;
        if (page.doc && page.doc.latest.version > parts.version) continue;
        const loaded = new Set(page.loaded);
        for (const key of missing) loaded.add(key);
        page = { ...page, loaded };
        update({ doc: page.doc ? show(change(page.doc, parts.change)) : page.doc, trouble: null });
      } catch (error) {
        if (stopped) return;
        update({ trouble: error.message });
        await sleep(1000);
      }
    }
  }

  /** Markup held back for a busy section shows once it is free, with no read needed. */
  async function flush() {
    let previous = "";
    while (!stopped) {
      await sleep(400);
      const state = page.doc;
      if (!state || state.shown === state.latest) continue;
      const { engaged, drafts } = busy();
      const keys = heldKeys(engaged, drafts, released, loading(), state.latest);
      const named = [...keys].sort().join("\n");
      if (keys.size && named === previous) continue;
      previous = named;
      const next = refresh(state, keys);
      settle(released, next.shown);
      if (next !== state) update({ doc: next });
    }
  }

  live();
  loadSections();
  flush();
  readPostponed();
  return {
    want(key) {
      if (!page.wanted.has(key)) update({ wanted: new Set(page.wanted).add(key) });
    },
    /** `docs::release`: the card shows the owner's answer until it shows one line more than `before`. */
    release(key, before) {
      released.set(key, Math.max(before, released.get(key) ?? before) + 1);
    },
    latest: () => page.doc?.latest,
    retry: wake,
    postponed: readPostponed,
    stop() {
      stopped = true;
      wake();
      window.removeEventListener("pagehide", leave);
      document.removeEventListener("freeze", leave);
      window.removeEventListener("pageshow", returned);
      document.removeEventListener("resume", returned);
    },
  };
}

/** What a section's components share: the path, the page's handle, the postponed cards, the address's anchor and whether the page may be out of date. */
const DocumentContext = createContext(null);

export default function DocumentPage({ route }) {
  const path = route.path.replace(/^\//, "");
  const [page, setPage] = useState(INITIAL);
  const live = useRef(null);
  useEffect(() => {
    const one = liveDocument(path, setPage);
    live.current = one;
    return one.stop;
  }, [path]);
  const [hash, setHash] = useState(hashTarget);
  useEffect(() => {
    const moved = () => setHash(hashTarget());
    window.addEventListener("hashchange", moved);
    return () => window.removeEventListener("hashchange", moved);
  }, []);
  const { doc, stale, trouble, missing, wanted, loaded } = page;
  const [marked, setMarked] = useState(false);
  const ready = !!doc && wanted.size > 0 && [...wanted].every((key) => loaded.has(key));
  useEffect(() => {
    if (ready && !marked) {
      setMarked(true);
      scrollAnchor();
      mark("document-ready");
    }
  }, [ready, marked]);
  const checking = stale && !!doc;
  const context = useMemo(() => ({ path, live: live.current, postponed: page.postponed, hash, checking }), [path, page.postponed, hash, checking, doc !== null]);
  const present = useMemo(() => new Set(doc?.latest.sections.map((section) => section.key) ?? []), [doc?.latest.sections]);
  const retry = () => live.current?.retry();
  const state = doc ? (marked ? "document" : "outline") : missing ? "missing" : trouble ? "failed" : "loading";
  return (
    <main>
      <div className="document-page" data-ready={state} data-feed={stale ? "stale" : "live"} aria-busy={String(!doc && !trouble && !missing)}>
        {doc ? (
          <DocumentContext.Provider value={context}>
            <DocumentBody document={doc.shown} present={present} anchors={page.anchors} failed={!!trouble} />
            <section className="asks-shell">
              <h2>Questions about this document</h2>
            </section>
          </DocumentContext.Provider>
        ) : missing ? (
          <Trouble title="Document not found" text={`There is no document at ${path}: it may have been moved, renamed or removed.`} back={[documentsHref(), "All documents"]} onRetry={retry} />
        ) : trouble ? (
          <Trouble title="The document could not be loaded" text={failed(trouble)} onRetry={retry} />
        ) : (
          <>
            <p role="status" className="status-text">
              Loading document…
            </p>
            <GhostDocument />
          </>
        )}
        <PageNotices reconnecting={checking} reconnect={RECONNECT} error={doc ? (trouble ?? "") : ""} />
      </div>
    </main>
  );
}

/** `trouble::failed`: the sentence for a read that did not finish. */
function failed(why) {
  const text = why.trim().replace(/\.+$/, "");
  return text ? `${text}.` : "The read did not finish. Try again.";
}

function Trouble({ title, text, back, onRetry }) {
  return (
    <section className="trouble card" role="status">
      <h1>{title}</h1>
      <p>{text}</p>
      <p className="trouble-actions">
        <Button onClick={onRetry}>Retry</Button>
        {back ? (
          <a className="trouble-back" href={href(back[0])}>
            <span aria-hidden="true">← </span>
            {back[1]}
          </a>
        ) : null}
      </p>
    </section>
  );
}

/** A document's body while it loads: its card, with headings and paragraphs in it. */
function GhostDocBody() {
  return (
    <div className="ghost-doc-body">
      <div className="ghost-heading">
        <span className="bone" style={{ "--w": "40%" }} />
      </div>
      <Skeleton lines={4} />
      <Skeleton lines={3} />
      <div className="ghost-heading">
        <span className="bone" style={{ "--w": "32%" }} />
      </div>
      <Skeleton lines={4} />
    </div>
  );
}

/** A document before its read: its head over its body's card, its outline beside it. */
function GhostDocument() {
  return (
    <div className="ghost-doc">
      <div className="ghost-doc-head">
        <div>
          <span className="bone" style={{ "--w": "22ch" }} />
        </div>
        <div className="ghost-title">
          <span className="bone" style={{ "--w": "58%" }} />
        </div>
        <div className="ghost-meta">
          <span className="bone" style={{ "--w": "5ch" }} /> <span className="bone" style={{ "--w": "7ch" }} /> <span className="bone" style={{ "--w": "8ch" }} />
        </div>
      </div>
      <div className="ghost-toc">
        <div>
          <span className="bone" style={{ "--w": "11ch" }} />
        </div>
        {Array.from({ length: 7 }, (_, n) => (
          <div key={n}>
            <span className="bone" />
          </div>
        ))}
      </div>
      <GhostDocBody />
    </div>
  );
}

/**
 * `viewport::Settle`: `children` hidden under `cover` until no region they reserve on the first
 * screen waits for its read, until `failed`, or for 3 s at most.
 */
function Settle({ failed: gaveUp, cover, children }) {
  const [settled, setSettled] = useState(false);
  const root = useRef(null);
  useEffect(() => {
    if (gaveUp) setSettled(true);
  }, [gaveUp]);
  useEffect(() => {
    if (settled) return;
    let drawn = null;
    let timer = null;
    const check = () => {
      const node = root.current;
      if (node) {
        drawn ??= Date.now();
        if (!waiting(node) || Date.now() - drawn > SETTLE_MS) return setSettled(true);
      }
      timer = setTimeout(check, 16);
    };
    timer = setTimeout(check, 16);
    return () => clearTimeout(timer);
  }, [settled]);
  return (
    <div className="settle" data-settle="docs" data-settled={String(settled)} aria-busy={String(!settled)} ref={root}>
      {settled ? null : <div className="settle-cover">{cover}</div>}
      {children}
    </div>
  );
}

/** Whether `root` has drawn nothing but its cover, or a region of it on the screen still waits. */
function waiting(root) {
  if (root.childElementCount < 2) return true;
  const page = root.getBoundingClientRect();
  const screen = [0, innerHeight, Math.max(page.left, 0), Math.min(page.right, innerWidth)];
  for (const region of root.querySelectorAll("[data-pending=true]")) {
    const box = region.getBoundingClientRect();
    if (box.bottom > box.top && box.right > box.left && box.bottom > screen[0] && box.top < screen[1] && box.right > screen[2] && box.left < screen[3]) return true;
  }
  return false;
}

/** `DocumentWithTasks`: the content, and beside it the related open tasks and the finished ones folded. */
function DocumentWithTasks({ tasks, children }) {
  const related = tasks.filter((task) => !task.slug.startsWith("verdicts/"));
  const finished = related.filter((task) => task.status === "done" || task.status === "cancelled");
  const open = related.filter((task) => task.status !== "done" && task.status !== "cancelled");
  return (
    <div className="document-content-layout">
      <div className="document-content">{children}</div>
      {related.length ? (
        <aside className="doc-tasks" aria-label="Related tasks">
          <h2>Related tasks</h2>
          {open.length ? (
            <ul className="doc-tasks-open">
              {open.map((task) => (
                <RelatedTask key={task.slug} task={task} />
              ))}
            </ul>
          ) : null}
          {finished.length ? (
            <details className="doc-tasks-finished">
              <summary>{finished.length} finished</summary>
              <ul>
                {finished.map((task) => (
                  <RelatedTask key={task.slug} task={task} />
                ))}
              </ul>
            </details>
          ) : null}
        </aside>
      ) : null}
    </div>
  );
}

function RelatedTask({ task }) {
  return (
    <Near height={40} listItem>
      <a href={href(`/task/${task.slug}`)}>{task.title}</a>
      <span className="doc-task-meta">
        <code className="doc-task-slug">{task.slug}</code>
        {" · "}
        <span className="doc-task-status">{task.status.replaceAll("_", " ")}</span>
      </span>
    </Near>
  );
}

function DocumentBody({ document, present, anchors, failed: gaveUp }) {
  const [wide] = useState(() => innerWidth > 1100);
  const { sections, cards, threads } = document;
  return (
    <DocumentWithTasks tasks={document.tasks}>
      <div className="doc-layout">
        <div className="doc-head">
          <p className="eyebrow">{document.path}</p>
          <h1>{document.title}</h1>
          <nav className="doc-tools" aria-label="Document tools">
            <a href={href("/")}>Home</a>
            <a href={href(documentsHref())}>All docs</a>
            <details className="versions">
              <summary>Versions and diffs</summary>
            </details>
          </nav>
        </div>
        {document.headings.length >= TOC_MIN_HEADINGS ? <Outline headings={document.headings} open={wide} /> : null}
        {/* The head and the outline are in with the document; its body waits under its ghost until the sections on the first screen are in. */}
        <Settle failed={gaveUp} cover={<GhostDocBody />}>
          <article className="doc-body">
            {document.comparison ? (
              <aside className="document-comparison">
                Differences from main belong to <a href={href(`/task/${document.comparison.task}`)}>{document.comparison.task}</a>
              </aside>
            ) : null}
            {sections.map((section, index) => {
              const slug = section.key.startsWith("card:") ? section.key.slice("card:".length) : null;
              return (
                <SectionSlot
                  key={section.key}
                  section={section}
                  first={index === 0 || sections[index - 1].part !== section.part}
                  last={index === sections.length - 1 || sections[index + 1].part !== section.part}
                  ids={anchors[section.key] ?? NONE}
                  gone={!present.has(section.key)}
                  card={slug ? cards.find((card) => card.slug === slug) : undefined}
                  thread={slug ? threads[slug] : undefined}
                />
              );
            })}
          </article>
        </Settle>
      </div>
    </DocumentWithTasks>
  );
}

const NONE = [];

const Outline = memo(function Outline({ headings, open }) {
  return (
    <nav className="doc-toc" aria-label="On this page">
      <details open={open}>
        <summary>On this page</summary>
        <div className="toc-links">
          {headings.map((heading) => (
            <Near key={heading.anchor} height={28}>
              <a href={`#${heading.anchor}`} className={`level-${heading.level}`}>
                {heading.text}
              </a>
            </Near>
          ))}
        </div>
      </details>
    </nav>
  );
});

/** A section's place: its reserved height until it nears the screen, then the section. */
const SectionSlot = memo(function SectionSlot({ section, first, last, ids, gone, card, thread }) {
  const anchors = section.start > 0 ? [...ids, `L${section.start}`] : ids;
  return (
    <Near className={`doc-part${first ? " doc-part-start" : ""}${last ? " doc-part-end" : ""}`} anchors={anchors} height={section.key.startsWith("card:") ? 100 : 160}>
      <HydratedSection section={section} card={card} thread={thread} gone={gone} ids={ids} />
    </Near>
  );
});

/** A mounted section: asks for its body, draws its ghost until it comes, or a settled card folded. */
function HydratedSection({ section, card, thread, gone, ids, unfolded = false }) {
  const { live } = useContext(DocumentContext);
  const key = section.key;
  const fold = !unfolded && settledGroup(card);
  useEffect(() => {
    if (!fold) live.want(key);
  }, [fold, key, live]);
  const ready = (section.blocks ?? []).length > 0;
  useEffect(() => {
    const target = hashTarget();
    if (ready && target && ids.includes(target)) scrollAnchor();
  }, [ready, ids]);
  // The outer slot owns the comparison even while a settled card is closed.
  const comparison = unfolded ? null : section.comparison;
  let content;
  if (fold) {
    const summary = section.blocks?.find((block) => block.type === "settled-card")?.summary;
    content = (
      <div className="doc-section folded-slot" id={`L${section.start}`}>
        <details className="settled" id={card.slug}>
          <summary>
            {summary ? (
              <Spans spans={summary} />
            ) : (
              <>
                <code>{card.slug}</code> · {card.state} · {card.text}
              </>
            )}
            {ids
              .filter((id) => id !== card.slug)
              .map((id) => (
                <span key={id} id={id} />
              ))}
          </summary>
          <Near>
            <HydratedSection section={section} card={card} thread={thread} gone={gone} ids={ids} unfolded />
          </Near>
        </details>
      </div>
    );
  } else if (ready && section.comparison?.kind === "removed") {
    content = <HistoricalBlocks blocks={section.blocks} />;
  } else if (ready) {
    content = <DocSection section={section} card={card} thread={thread} gone={gone} unfolded={unfolded} />;
  } else {
    content = (
      <div className="section-loading" id={section.start > 0 ? `L${section.start}` : undefined} data-pending="true">
        {ids.map((id) => (
          <span key={id} id={id} />
        ))}
        {/* A card's box with its lines in it; a section's heading over its first paragraph. */}
        {card ? (
          <div className="ghost-box">
            <Skeleton lines={3} label="Loading section…" />
          </div>
        ) : (
          <>
            <div className="ghost-heading">
              <span className="bone" style={{ "--w": "36%" }} />
            </div>
            <Skeleton lines={4} label="Loading section…" />
          </>
        )}
      </div>
    );
  }
  return <SectionDifference comparison={comparison}>{content}</SectionDifference>;
}

const DIFFERENCES = { added: "Added", changed: "Changed", removed: "Removed" };

function SectionDifference({ comparison, children }) {
  if (!comparison) return children;
  const { kind } = comparison;
  return (
    <div className={`section-difference difference-${kind}`} data-difference={kind}>
      <p className="difference-label" title="Compared with main">
        {DIFFERENCES[kind]}
      </p>
      {comparison.before?.length ? (
        <details className="difference-before">
          <summary>Previously on main</summary>
          <Near>
            <HistoricalBlocks blocks={comparison.before} />
          </Near>
        </details>
      ) : null}
      {children}
    </div>
  );
}

/** A link's excerpt, drawn beside the button that opened it: the section its anchor names. */
function Excerpt({ target }) {
  const [read, setRead] = useState(null);
  const anchor = target.anchor ?? "";
  const title = `${target.path}#${anchor}`;
  useEffect(() => {
    let current = true;
    get(`/api/section?path=${encode(target.path.replace(/^\/+/, ""))}&format=blocks&anchor=${encode(anchor)}`).then(
      (fragment) => current && setRead({ fragment }),
      (error) => current && setRead({ error: error.message }),
    );
    return () => {
      current = false;
    };
  }, [target.path, anchor]);
  return (
    <div className="excerpt-box" role="region" aria-label={`Excerpt of ${title}`} data-busy="true">
      <p className="excerpt-title">{title}</p>
      {!read ? (
        <p role="status">Loading the excerpt…</p>
      ) : read.error ? (
        <p role="alert">{read.error}</p>
      ) : !read.fragment.blocks?.length ? (
        <p>This section is empty.</p>
      ) : (
        <HistoricalBlocks blocks={read.fragment.blocks} />
      )}
    </div>
  );
}

const excerpt = (target) => <Excerpt target={target} />;

/** A section drawn: its prose, a card's thread, its actions and its editor. */
const DocSection = memo(function DocSection({ section, card, thread, gone, unfolded }) {
  const { path, postponed: later, hash } = useContext(DocumentContext);
  const [editing, setEditing] = useState(false);
  const settled = settledGroup(card);
  let blocks = section.blocks;
  const summary = blocks.find((block) => block.type === "settled-card")?.summary ?? NONE;
  if (settled && blocks.length === 1 && blocks[0].type === "settled-card") blocks = blocks[0].blocks;
  const slug = card?.slug;
  const isCard = !!card;
  const postponed = !!slug && later.includes(slug);
  const controls =
    card && (card.group === "decide" || card.group === "confirm") ? (
      <Later slug={card.slug} postponed={postponed} seen={editing || hash === card.slug ? `${hash}:${editing}` : null} />
    ) : null;
  // The button that opens the editor, where Escape returns focus.
  const opener = `opener-${encode(`${path}:${section.key}`)}`;
  const content = (
    <ExcerptContext.Provider value={isCard ? excerpt : null}>
      <CardAnchorContext.Provider value={unfolded ? slug : null}>
        <div
          className="section-prose"
          onClick={(event) => {
            const interactive = event.target.closest?.("a, button, input, summary, textarea, img, iframe, .viewer-stage, .excerpt-box");
            if (isCard && !interactive) setEditing(true);
          }}
        >
          <Blocks blocks={blocks} />
        </div>
      </CardAnchorContext.Provider>
      {thread ? (
        <Near height={100}>
          <Blocks blocks={thread.blocks} />
        </Near>
      ) : null}
      {isCard ? (
        <div className="doc-actions">
          {postponed ? <small className="postponed">postponed</small> : null}
          <Button id={opener} aria-expanded={editing} onClick={() => setEditing((open) => !open)}>
            {editing ? "Close" : "Verdict / message"}
          </Button>
        </div>
      ) : (
        <Button id={opener} className="dx-button add" aria-expanded={editing} onClick={() => setEditing((open) => !open)}>
          {editing ? "Close comment" : "+ comment"}
        </Button>
      )}
      {gone ? <p role="alert">This block was removed elsewhere. Copy your draft or discard it; saving to its old position is disabled.</p> : null}
      <Editor sectionKey={section.key} slug={slug} state={card?.state ?? ""} afterLine={section.end} editing={editing} setEditing={setEditing} opener={opener} gone={gone} later={controls} />
    </ExcerptContext.Provider>
  );
  return (
    <section
      className={isCard ? "doc-section doc-card" : "doc-section"}
      id={unfolded ? undefined : `L${section.start}`}
      data-state={card?.state}
      data-doc-key={section.key}
      data-start={section.start}
      data-end={section.end}
      data-busy={String(editing)}
    >
      {settled && !unfolded ? (
        <details className="settled" id={slug}>
          <summary>
            <Spans spans={summary} />
          </summary>
          <Near>{content}</Near>
        </details>
      ) : (
        content
      )}
    </section>
  );
});

/** `postpone_live::Controls`: Later or Undo Later, and the card marked seen once its editor opens. */
function Later({ slug, postponed, seen }) {
  const { path, live, checking } = useContext(DocumentContext);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(null);
  useEffect(() => {
    if (seen == null) return;
    post("/api/postpone", { path, slug, action: "seen" }).catch((why) => setError(why.message));
  }, [seen, path, slug]);
  const label = pending ? "Saving…" : checking ? CHECKING : postponed ? "Undo Later" : "Later";
  const press = async () => {
    if (pending || checking) return;
    setPending(true);
    setError(null);
    try {
      const value = await post("/api/postpone", { path, slug, action: postponed ? "undo" : "later" });
      live.postponed();
      if (!postponed && typeof value?.next === "string") location.href = href(value.next);
    } catch (why) {
      setError(why.message);
    }
    setPending(false);
  };
  return (
    <span className="postpone-controls">
      {postponed ? <small className="postponed">postponed</small> : null}
      <Button disabled={pending || checking} title="Return after the rest of this group, or in 3 days" onClick={press}>
        {label}
      </Button>
      {error ? <span role="alert">{error}</span> : null}
    </span>
  );
}

/** `symphony_api::UPLOAD_ACCEPT`. */
const ACCEPT =
  "image/png,image/jpeg,image/gif,image/webp,application/pdf,text/plain,text/markdown,text/csv,application/json,.png,.jpg,.jpeg,.gif,.webp,.pdf,.txt,.md,.csv,.json,.log,.docx,.xlsx,.pptx";

/** The attach module's style, put in the page by the first composer to mount (`attach::style`). */
function attachStyle() {
  if (document.getElementById("attach-style")) return;
  const style = document.createElement("style");
  style.id = "attach-style";
  style.textContent = ATTACH_STYLE;
  document.head.append(style);
}

/** `attach::AttachControls` as drawn with no file attached (attachments are left out). */
function AttachControls({ disabled }) {
  return (
    <div className="attach" data-attach-off={String(disabled)}>
      <label className={disabled ? "attach-pick disabled" : "attach-pick"} title="Attach pictures or documents: pick them, paste a screenshot or drop files here">
        <input className="dx-input" type="file" multiple accept={ACCEPT} disabled={disabled} data-attach-input="true" />
        <span aria-hidden="true">📎</span> Attach
      </label>
      <span className="attach-hint">or paste, or drop files here</span>
    </div>
  );
}

/** What the editor says once the server has `action` (a verdict, `message` or `comment`). */
function savedText(action) {
  return action === "message" ? "Message sent" : action === "comment" ? "Comment saved" : `Verdict saved: ${action}`;
}

/** `docs::ending`: only the words the server holds are cleared or said to be saved. */
function ending(held, last, boxed) {
  const unchanged = boxed === last.note;
  if (held && unchanged && held.action === last.action && held.note === last.note) return { status: savedText(held.action), clear: true, close: true };
  if (held) {
    return {
      status: unchanged ? `${savedText(held.action)}; your change was not sent: send it again to add it` : savedText(held.action),
      clear: false,
      close: unchanged,
    };
  }
  return { status: "Verdict saved; the card shows its words: send yours again if they are not there", clear: false, close: unchanged };
}

const CHOICES = [
  ["approved", "✓ Approve"],
  ["rejected", "✗ Reject"],
  ["question", "? Question"],
];
const KINDS = ["comment", "question", "decision"].map((value) => [value, value]);

/** A section's editor: a card's verdict and message form, or a prose section's comment form. */
function Editor({ sectionKey, slug, state, afterLine, editing, setEditing, opener, gone, later }) {
  const { path, live, checking } = useContext(DocumentContext);
  const key = `review-draft:${path}:${sectionKey}`;
  const fieldId = `draft-${encode(key)}`;
  const [text, setText] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(null);
  const [saved, setSaved] = useState(null);
  const box = useRef(text);
  box.current = text;
  const sending = useRef(false);
  const message = slug != null;
  useEffect(attachStyle, []);
  useEffect(() => {
    if (editing) document.getElementById(fieldId)?.focus();
  }, [editing, fieldId]);

  const save = async (action) => {
    if (gone) return setError("This block was removed; your draft is kept.");
    // While the page may show what is out of date, the card may have moved since: wait for it.
    if (sending.current || checking) return;
    // A comment's first line is its title, so it needs words, as a message does.
    if (!text.trim() && (action === "comment" || action === "message")) return setError("Type a message first");
    const note = text;
    sending.current = true;
    setPending(true);
    setError(null);
    setSaved(null);
    // The card's lines of the owner's as the save is sent: the feed may bring its change first.
    const before = message ? ownerLines(live.latest(), sectionKey) : 0;
    let route;
    const fields = { path, text: note };
    if (message) {
      fields.slug = slug;
      if (action === "message") route = "/api/message";
      else {
        Object.assign(fields, { state: action, note, tap: `ui-${Date.now()}-${Math.random()}` });
        route = "/api/decision";
      }
    } else {
      Object.assign(fields, { after_line: afterLine, kind: "comment" });
      route = "/api/comment";
    }
    const words = { action, note };
    try {
      const reply = await post(route, fields);
      // A tap the server had written already: no new line comes, and the reply says its words.
      const repeat = reply?.repeat === true;
      const held = repeat ? (typeof reply.state === "string" && typeof reply.note === "string" ? { action: reply.state, note: reply.note } : null) : words;
      if (message && !repeat) live.release(sectionKey, before);
      const end = ending(held, words, box.current);
      if (end.clear) setText("");
      setSaved(end.status);
      if (end.close) setEditing(false);
    } catch (why) {
      setError(why.message);
    }
    sending.current = false;
    setPending(false);
  };
  const submit = () => save(message ? "message" : "comment");
  const held = (label) => (checking ? CHECKING : label);
  return (
    <div className="editor" data-busy={String(editing || pending || text !== "")} data-draft={String(!editing && !pending && text !== "")}>
      {text !== "" && !editing ? <Button onClick={() => setEditing(true)}>Resume draft</Button> : null}
      <form
        className={message ? "verdict" : "comment"}
        hidden={!editing}
        data-attach={key}
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        onKeyDown={(event) => {
          // Escape closes the editor, its draft kept, and returns focus to its opener.
          if (event.key === "Escape") {
            event.preventDefault();
            setEditing(false);
            document.getElementById(opener)?.focus();
          }
        }}
      >
        <Textarea
          id={fieldId}
          value={text}
          disabled={pending}
          aria-label="Message or verdict note"
          placeholder="Write a message, or add a note to your verdict…"
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              submit();
            }
          }}
        />
        <AttachControls disabled={pending} />
        {message ? (
          // Approve is what the owner most often does, so it alone is filled; the card's own verdict is marked as current.
          <div className="verdict-choices">
            {CHOICES.map(([value, label]) => (
              <Button key={value} className={value === "approved" ? "dx-button v-approved primary" : `dx-button v-${value}`} aria-current={value === state ? "true" : undefined} disabled={pending || checking} onClick={() => save(value)}>
                {held(label)}
              </Button>
            ))}
            {later}
          </div>
        ) : null}
        <div className="row">
          {!message ? <SelectTrigger label="Comment kind" value="comment" options={KINDS} /> : null}
          <Button type="submit" className={message ? "dx-button" : "dx-button primary"} disabled={pending || checking}>
            {pending ? "Saving…" : checking ? CHECKING : message ? "Send message" : "Save comment"}
          </Button>
          {message ? (
            <>
              <Button className="dx-button v-dismissed" aria-current={state === "dismissed" ? "true" : undefined} disabled={pending || checking} onClick={() => save("dismissed")}>
                {held("⊘ Dismiss")}
              </Button>
              {/* A pending card has no verdict to take back. */}
              {state !== "pending" ? (
                <Button variant="ghost" className="dx-button reset" title="Take back the verdict: the card waits for yours again" disabled={pending || checking} onClick={() => save("pending")}>
                  {held("↺ Reset to pending")}
                </Button>
              ) : null}
            </>
          ) : null}
          <Button
            variant="ghost"
            className="dx-button discard"
            title="Clear what you wrote and close"
            disabled={pending}
            onClick={() => {
              setText("");
              setEditing(false);
            }}
          >
            Discard draft
          </Button>
        </div>
      </form>
      {error ? <p role="alert">{error}</p> : null}
      {saved ? <p role="status">{saved}</p> : null}
    </div>
  );
}
