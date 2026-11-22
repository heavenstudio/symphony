// A document's, a folder's and an HTML document's pages, as the Dioxus client's docs_live.rs and
// html_page.rs drew them. Typed prose stays in keyed components; drafts live outside that markup
// (editor.tsx). What a page reads and follows is its model's (document-live.ts), which the page
// draws each time it changes; a section that did not change draws nothing again.

import {
  type CSSProperties,
  createContext,
  memo,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useState,
  useSyncExternalStore,
} from "react";
import { Asks } from "./asks";
import {
  BlocksView,
  CardAnchorContext,
  ExcerptsContext,
  HistoricalBlocks,
  SpansView,
} from "./blocks";
import { Button } from "./button";
import { type Item, shown } from "./cache";
import { documentItem } from "./cache-document";
import { watchContents } from "./contents";
import { folderPill, isSettled, partEdges, partForm } from "./document";
import { api, DocumentLive, documentLive, FolderLive, followFolder, named } from "./document-live";
import { readDiff, readFragment, readVersions } from "./document-read";
import { Editor, PageContext } from "./editor";
import { FirstSteps } from "./first";
import { useFrame, useRefreshed } from "./frame";
import { appPath } from "./links";
import { PageNotices } from "./live";
import { useMount } from "./mount";
import { PostponeControls } from "./postpone";
import { documentsHref, entrypoints, fileAddress, href, project, scoped } from "./project";
import { ComparisonNote, DocumentWithTasks, SectionDifference } from "./related";
import { getJson, said } from "./request";
import { anchor, scrollAnchor, useAnchor } from "./route";
import { failed as failedRead, ready as readyWord, Trouble } from "./trouble";
import { AtOnce, NearViewport, Settle, Skeleton } from "./viewport";
import type {
  Document,
  DocumentCard,
  DocumentDiff,
  Folder,
  Fragment,
  LinkTarget,
  Section,
  Version,
} from "./wire";
import { PAGE_NAMES } from "./wire";

/** The original reader reserves a contents column only for three or more headings. */
const TOC_MIN_HEADINGS = 3;

/** What a page's feed says while it reconnects. */
const RECONNECT = "Reconnecting: this document may be out of date";

const NONE: string[] = [];

/** A bone of a ghost, `width` wide. */
const bone = (width: string) => ({ "--w": width }) as CSSProperties;

/** `path` without the checkout `scope`'s prefix (`@SCOPE/`), as a page's head shows it. */
function unscoped(path: string, scope: string): string {
  const prefix = `@${scope}/`;
  return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

/** What a folder's intro tells the folder's page: its feed, its trouble and its device copy. */
export interface IntroNotice {
  stale: boolean;
  error: string;
  removed: boolean;
  /** What the intro shows from this device (`shown`, cache.ts). */
  device: [number, boolean] | null;
}

const QUIET: IntroNotice = { stale: false, error: "", removed: false, device: null };

/** Where a folder's intro says what its page's notices show; none on a document's own page. */
const FolderNoticeContext = createContext<((notice: IntroNotice) => void) | null>(null);

/**
 * The item an entrypoint's folder page keeps on this device, pinned, which its intro keeps its own
 * reads in; none for any other folder page, which keeps nothing, its intro included.
 */
const KeptFolderContext = createContext<Item | null>(null);

export interface DocumentPageProps {
  /** The document's route, under its checkout. */
  path: string;
  /** The checkout the route names, empty for the main one's. */
  scope: string;
  /** A folder's intro, drawn on its folder's page: no head, outline, asks or notices of its own. */
  embedded?: boolean;
}

/**
 * The routes of the server's answer to where a document may have moved (`/api/moved`,
 * `docview::moved`): each a route a page may link, as a path of the app; none for any other answer.
 */
export function readMoved(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter(
    (route): route is string => typeof route === "string" && appPath(`/${route}`),
  );
}

/** The address of the document `route` in the checkout `scope`, the fragment `hash` (`#…`) kept. */
export function movedAddress(scope: string, route: string, hash: string): string {
  return `${href(scoped(scope, `/${route}`))}${hash}`;
}

/**
 * Open `address` in this page's place, as the shell follows a link, but in the route's place in
 * the history: Back leaves the route that has no document.
 */
function follow(address: string): void {
  history.replaceState(null, "", address);
  dispatchEvent(new PopStateEvent("popstate"));
}

/**
 * Where the document the server has none of at `path` may be, once it said so (`asking`): the
 * documents of its name (`/api/moved`), null until that read answered. The one document of its
 * name is opened in this page's place, the address's fragment kept, as a link filed before its
 * document moved names it; more than one is the reader's choice; a failed read finds none.
 */
function useMoved(path: string, scope: string, asking: boolean): string[] | null {
  const [moved, setMoved] = useState<string[] | null>(null);
  useEffect(() => {
    setMoved(null);
    if (!asking) return;
    const reading = new AbortController();
    getJson(api(scope, "/api/moved", path), reading.signal).then(
      (value) => {
        if (reading.signal.aborted) return;
        const routes = readMoved(value);
        const [only] = routes;
        if (routes.length === 1 && only !== undefined) {
          follow(movedAddress(scope, only, location.hash));
        } else {
          setMoved(routes);
        }
      },
      () => {
        if (!reading.signal.aborted) setMoved([]);
      },
    );
    return () => reading.abort();
  }, [path, scope, asking]);
  return moved;
}

export interface MissingProps {
  /** The route the page found no document at. */
  path: string;
  scope: string;
  /** The documents of its name ([`readMoved`]): more than one is offered to choose. */
  moved: string[];
  /** The address's fragment (`#…`), which each choice keeps. */
  hash: string;
  retry: () => void;
}

/**
 * What a page that has no document at its route says: Document not found, with All documents and
 * Retry, and, when documents share the route's name, each of them to choose rather than the first.
 */
export function Missing({ path, scope, moved, hash, retry }: MissingProps) {
  const name = moved[0]?.split("/").pop() ?? "";
  return (
    <Trouble
      title="Document not found"
      text={`There is no document at ${path}: it may have been moved, renamed or removed.`}
      back={[scoped(scope, documentsHref(project())), "All documents"]}
      retry={retry}
    >
      {moved.length > 1 && (
        <>
          <p>{`It may be one of the documents named ${name}:`}</p>
          <ul className="trouble-choices">
            {moved.map((route) => (
              <li key={route}>
                <a href={movedAddress(scope, route, hash)}>{route}</a>
              </li>
            ))}
          </ul>
        </>
      )}
    </Trouble>
  );
}

/** A document's page: what this device kept of it at once, then the server's, then its feed. */
export function DocumentPage({ path, scope, embedded = false }: DocumentPageProps) {
  const frame = useFrame();
  const refreshed = useRefreshed();
  const notice = useContext(FolderNoticeContext);
  const folderItem = useContext(KeptFolderContext);
  // An intro keeps its reads in its entrypoint folder's item; a document page, in its own.
  const [page] = useState(
    () =>
      new DocumentLive(
        path,
        scope,
        embedded ? folderItem : documentItem(named(scope, path), false),
        frame,
      ),
  );
  useSyncExternalStore(page.subscribe, page.snapshot, page.snapshot);
  useEffect(() => {
    const reading = new AbortController();
    void documentLive(page, reading.signal);
    return () => reading.abort();
  }, [page]);
  // The cards postponed in the checkout, read again with the frame and as the queue moves.
  const [postponed, setPostponed] = useState<string[]>([]);
  const orders = page.orders;
  useEffect(() => {
    void refreshed;
    void orders;
    const reading = new AbortController();
    getJson(scoped(scope, "/api/postponed"), reading.signal).then(
      (cards) => {
        if (reading.signal.aborted) return;
        if (Array.isArray(cards) && cards.every((card) => typeof card === "string")) {
          setPostponed(cards);
        }
      },
      () => {},
    );
    return () => reading.abort();
  }, [scope, refreshed, orders]);
  const state = page.state;
  const device = shown(page.freshness);
  const trouble = page.trouble;
  // An intro the server no longer has follows no feed: the folder's next read drops it.
  const [deviceAt, unreachable] = device ?? [null, false];
  const removed = state?.removed ?? false;
  const introStale = page.stale && !page.missing;
  // A document the server has none of is looked for by its name before the page says so.
  const moved = useMoved(path, scope, page.missing && !embedded);
  const missing = page.missing && (embedded || moved !== null);
  useEffect(() => {
    if (!embedded || notice === null) return;
    notice({
      stale: introStale,
      error: trouble ?? "",
      removed,
      device: deviceAt === null ? null : [deviceAt, unreachable],
    });
  }, [embedded, notice, introStale, trouble, removed, deviceAt, unreachable]);
  const [marked, setMarked] = useState(false);
  const complete = state !== null && page.ready();
  useEffect(() => {
    if (complete && !marked) {
      setMarked(true);
      scrollAnchor();
      performance.mark("document-ready");
    }
  }, [complete, marked]);
  const retry = () => {
    page.retry.current += 1;
  };
  const ready =
    state !== null ? (marked ? "document" : "outline") : readyWord(missing, trouble !== null);
  let body: ReactNode = null;
  if (state !== null) {
    body = (
      <>
        <DocumentBody
          document={state.shown}
          latest={state.latest}
          scope={scope}
          anchors={page.anchors}
          embedded={embedded}
          failed={trouble !== null}
          postponed={postponed}
        />
        {!embedded && <Asks key={`${scope}/${path}`} path={path} scope={scoped(scope, "")} />}
      </>
    );
  } else if (missing) {
    // A folder's intro shows nothing in its place: the folder is still there.
    if (!embedded) {
      body = (
        <Missing
          path={path}
          scope={scope}
          moved={moved ?? NONE}
          hash={location.hash}
          retry={retry}
        />
      );
    }
  } else if (trouble !== null && !embedded) {
    body = (
      <Trouble
        title="The document could not be loaded"
        text={failedRead(trouble, true)}
        retry={retry}
      />
    );
  } else if (trouble === null || (embedded && notice === null)) {
    body = (
      <>
        <p role="status" className={trouble === null ? "status-text" : undefined}>
          {trouble ?? "Loading document…"}
        </p>
        {trouble === null && <GhostDocument toc={!embedded} head={!embedded} />}
      </>
    );
  }
  return (
    <PageContext.Provider value={page}>
      <div
        className="document-page"
        data-ready={ready}
        data-feed={page.stale ? "stale" : "live"}
        data-cache={device ? "device" : undefined}
        aria-busy={`${state === null && trouble === null && !missing}`}
      >
        {body}
        {/* Until the document is shown, what kept it away is the block above's to say, once. */}
        {!embedded && (
          <PageNotices
            reconnecting={page.stale && state !== null}
            reconnect={RECONNECT}
            error={state !== null ? (trouble ?? "") : ""}
            documentRemoved={removed}
            device={device}
            composer
          />
        )}
      </div>
    </PageContext.Provider>
  );
}

/** A document's body while it loads: its card, with headings and paragraphs in it. */
export function GhostDocBody() {
  return (
    <div className="ghost-doc-body">
      <div className="ghost-heading">
        <span className="bone" style={bone("40%")} />
      </div>
      <Skeleton lines={4} />
      <Skeleton lines={3} />
      <div className="ghost-heading">
        <span className="bone" style={bone("32%")} />
      </div>
      <Skeleton lines={4} />
    </div>
  );
}

/**
 * A document before its read: its head (path, title and tools) over its body's card, and beside it
 * on a wide screen its outline (`toc`).
 */
export function GhostDocument({ toc, head }: { toc: boolean; head: boolean }) {
  return (
    <div className="ghost-doc">
      {head && (
        <div className="ghost-doc-head">
          <div>
            <span className="bone" style={bone("22ch")} />
          </div>
          <div className="ghost-title">
            <span className="bone" style={bone("58%")} />
          </div>
          <div className="ghost-meta">
            <span className="bone" style={bone("5ch")} />{" "}
            <span className="bone" style={bone("7ch")} />{" "}
            <span className="bone" style={bone("8ch")} />
          </div>
        </div>
      )}
      {toc && (
        <div className="ghost-toc">
          <div>
            <span className="bone" style={bone("11ch")} />
          </div>
          {Array.from({ length: 7 }, (_, n) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: the outline's ghost lines are alike
            <div key={n}>
              <span className="bone" />
            </div>
          ))}
        </div>
      )}
      <GhostDocBody />
    </div>
  );
}

interface DocumentBodyProps {
  /** The copy shown. */
  document: Document;
  /** The feed's latest copy, which tells a section gone from it. */
  latest: Document;
  scope: string;
  anchors: Record<string, string[]>;
  embedded: boolean;
  failed: boolean;
  postponed: readonly string[];
}

/**
 * The document drawn: its head and outline, then its body under its ghost until the sections on
 * the first screen are in, beside its related tasks.
 */
export const DocumentBody = memo(function DocumentBody({
  document,
  latest,
  scope,
  anchors,
  embedded,
  failed,
  postponed,
}: DocumentBodyProps) {
  const path = document.path;
  const [wide] = useState(() => typeof innerWidth === "undefined" || innerWidth > 1100);
  const toc = !embedded && document.headings.length >= TOC_MIN_HEADINGS;
  useEffect(() => {
    if (!toc) return;
    const watching = new AbortController();
    watchContents(watching.signal);
    return () => watching.abort();
  }, [toc]);
  const present = new Set(latest.sections.map((section) => section.key));
  return (
    <DocumentWithTasks tasks={document.tasks}>
      <div className="doc-layout">
        {!embedded && (
          <div className="doc-head">
            <p className="eyebrow">{unscoped(document.path, scope)}</p>
            <h1>{document.title}</h1>
            <nav className="doc-tools" aria-label="Document tools">
              <a href={href("/")}>{PAGE_NAMES.HOME}</a>
              <a href={href(scoped(scope, documentsHref(project())))}>All docs</a>
              <Versions path={path} scope={scope} />
            </nav>
          </div>
        )}
        {toc && (
          <nav className="doc-toc" aria-label="On this page">
            <details open={wide}>
              <summary>On this page</summary>
              <div className="toc-links">
                {document.headings.map((heading) => (
                  <NearViewport key={heading.anchor} height={28}>
                    <a href={`#${heading.anchor}`} className={`level-${heading.level}`}>
                      {heading.text}
                    </a>
                  </NearViewport>
                ))}
              </div>
            </details>
          </nav>
        )}
        {/* The head and the outline are in with the document; its body waits under its ghost
            until the sections on the first screen are in. */}
        <Settle failed={failed} cover={<GhostDocBody />}>
          <article className="doc-body">
            {document.comparison != null && <ComparisonNote comparison={document.comparison} />}
            {document.sections.map((section, index) => {
              const [first, last] = partEdges(document.sections, index);
              const ids = Object.hasOwn(anchors, section.key)
                ? (anchors[section.key] ?? NONE)
                : NONE;
              const slug = section.key.startsWith("card:") ? section.key.slice(5) : null;
              const card =
                slug === null
                  ? null
                  : (document.cards.find((listed) => listed.slug === slug) ?? null);
              return (
                <NearViewport
                  key={section.key}
                  className={`doc-part${first ? " doc-part-start" : ""}${last ? " doc-part-end" : ""}${partForm(section, card)}`}
                  anchors={section.start > 0 ? [...ids, `L${section.start}`] : ids}
                  height={slug !== null ? 100 : 160}
                >
                  <HydratedSection
                    ids={ids}
                    gone={!present.has(section.key)}
                    section={section}
                    path={path}
                    scope={scope}
                    card={card}
                    thread={
                      slug !== null && Object.hasOwn(document.threads, slug)
                        ? (document.threads[slug] ?? null)
                        : null
                    }
                    postponed={slug !== null && postponed.includes(slug)}
                  />
                </NearViewport>
              );
            })}
          </article>
        </Settle>
      </div>
    </DocumentWithTasks>
  );
});

interface SectionProps {
  section: Section;
  path: string;
  scope: string;
  card: DocumentCard | null;
  thread: Fragment | null;
  /** The section is gone from the latest copy. */
  gone: boolean;
  /** The ids the section holds (`anchors`). */
  ids: string[];
  /** A settled card's body, drawn inside its fold. */
  unfolded?: boolean;
  /** The card is postponed in the review queue. */
  postponed: boolean;
}

/** The summary a settled card's section gives itself, if it has one. */
function settledSummary(section: Section) {
  for (const block of section.blocks) if (block.type === "settled-card") return block.summary;
  return null;
}

/**
 * A section of the document: its body once read (a settled card folded under its summary until
 * opened), its ghost until then, and its difference from main.
 */
export const HydratedSection = memo(function HydratedSection({
  section,
  path,
  scope,
  card,
  thread,
  gone,
  ids,
  unfolded = false,
  postponed,
}: SectionProps) {
  const page = useContext(PageContext);
  const key = section.key;
  const fold = !unfolded && isSettled(card);
  useEffect(() => {
    if (!fold) page?.want(key);
  }, [fold, key, page]);
  const ready = section.blocks.length > 0;
  useEffect(() => {
    const target = anchor();
    if (ready && target !== "" && ids.includes(target)) scrollAnchor();
  }, [ready, ids]);
  // The outer slot owns the comparison even while a settled card is closed. Its unfolded child
  // owns only the live content, so opening it never repeats the change's label or history.
  const comparison = unfolded ? null : section.comparison;
  let content: ReactNode;
  if (fold && card !== null) {
    const summary = settledSummary(section);
    content = (
      <div className="doc-section folded-slot" id={`L${section.start}`}>
        <details className="settled" id={card.slug}>
          <summary>
            {summary !== null ? (
              <SpansView spans={summary} />
            ) : (
              <>
                <code>{card.slug}</code>
                {` · ${card.state} · ${card.text}`}
              </>
            )}
            {ids
              .filter((id) => id !== card.slug)
              .map((id) => (
                <span key={id} id={id} />
              ))}
          </summary>
          <NearViewport>
            <HydratedSection
              section={section}
              path={path}
              scope={scope}
              card={card}
              thread={thread}
              gone={gone}
              ids={ids}
              unfolded
              postponed={postponed}
            />
          </NearViewport>
        </details>
      </div>
    );
  } else if (ready && section.comparison?.kind === "removed") {
    content = <HistoricalBlocks blocks={section.blocks} />;
  } else if (ready) {
    content = (
      <DocSection
        section={section}
        path={path}
        scope={scope}
        card={card}
        thread={thread}
        gone={gone}
        ids={ids}
        unfolded={unfolded}
        postponed={postponed}
      />
    );
  } else {
    content = (
      <div
        className="section-loading"
        id={section.start > 0 ? `L${section.start}` : undefined}
        data-pending="true"
      >
        {ids.map((id) => (
          <span key={id} id={id} />
        ))}
        {/* A card's box with its lines in it; a section's heading over its first paragraph. */}
        {card !== null ? (
          <div className="ghost-box">
            <Skeleton lines={3} label="Loading section…" />
          </div>
        ) : (
          <>
            <div className="ghost-heading">
              <span className="bone" style={bone("36%")} />
            </div>
            <Skeleton lines={4} label="Loading section…" />
          </>
        )}
      </div>
    );
  }
  return <SectionDifference comparison={comparison}>{content}</SectionDifference>;
});

/** What a click on a card's prose leaves alone: what it lands on does something of its own. */
const INTERACTIVE = "a, button, input, summary, textarea, img, iframe, .viewer-stage, .excerpt-box";

/** A section with its body: its prose, a card's thread, and the editor that answers it. */
function DocSection({
  section,
  path,
  scope,
  card,
  thread,
  gone,
  unfolded = false,
  postponed,
}: SectionProps) {
  const [editing, setEditing] = useState(false);
  const settled = isSettled(card);
  const summary = settledSummary(section) ?? [];
  const only = section.blocks.length === 1 ? section.blocks[0] : undefined;
  const blocks = settled && only?.type === "settled-card" ? only.blocks : section.blocks;
  const slug = card?.slug ?? null;
  const target = useAnchor();
  const later =
    card !== null && (card.group === "decide" || card.group === "confirm") ? (
      <PostponeControls
        path={path}
        scope={scope}
        slug={card.slug}
        postponed={postponed}
        seenAs={editing || target === card.slug ? `${target}:${editing}` : null}
      />
    ) : null;
  const excerpt = useCallback((link: LinkTarget) => <Excerpt target={link} />, []);
  // The button that opens the editor, where Escape returns focus.
  const opener = `opener-${encodeURIComponent(`${path}:${section.key}`)}`;
  const content = (
    <>
      {/* biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: a card's prose opens its editor on a click, as its button below does by the keyboard */}
      <div
        className="section-prose"
        onClick={(event) => {
          const target = event.target as Element;
          if (card !== null && target.closest?.(INTERACTIVE) == null) setEditing(true);
        }}
      >
        <BlocksView blocks={blocks} />
      </div>
      {thread !== null && (
        <NearViewport height={100}>
          <BlocksView blocks={thread.blocks} />
        </NearViewport>
      )}
      {card !== null ? (
        <div className="doc-actions">
          {postponed && <small className="postponed">postponed</small>}
          <Button
            id={opener}
            type="button"
            aria-expanded={editing}
            onClick={() => setEditing(!editing)}
          >
            {editing ? "Close" : "Verdict / message"}
          </Button>
        </div>
      ) : (
        <Button
          id={opener}
          className="add"
          type="button"
          aria-expanded={editing}
          onClick={() => setEditing(!editing)}
        >
          {editing ? "Close comment" : "+ comment"}
        </Button>
      )}
      {gone && (
        <p role="alert">
          This block was removed elsewhere. Copy your draft or discard it; saving to its old
          position is disabled.
        </p>
      )}
      <Editor
        path={path}
        scope={scope}
        sectionKey={section.key}
        slug={slug}
        state={card?.state ?? ""}
        afterLine={section.end}
        editing={editing}
        setEditing={setEditing}
        opener={opener}
        gone={gone}
        later={later}
      />
    </>
  );
  return (
    <ExcerptsContext.Provider value={card !== null ? excerpt : null}>
      <CardAnchorContext.Provider value={unfolded ? slug : null}>
        <section
          className={card !== null ? "doc-section doc-card" : "doc-section"}
          id={unfolded ? "" : `L${section.start}`}
          data-state={card?.state}
          data-doc-key={section.key}
          data-start={section.start}
          data-end={section.end}
          data-busy={editing ? "true" : "false"}
        >
          {settled && !unfolded && card !== null ? (
            <details className="settled" id={card.slug}>
              <summary>
                <SpansView spans={summary} />
              </summary>
              <NearViewport>{content}</NearViewport>
            </details>
          ) : (
            content
          )}
        </section>
      </CardAnchorContext.Provider>
    </ExcerptsContext.Provider>
  );
}

/**
 * A link's excerpt, drawn beside the button that opened it: the section its anchor names. Its cards
 * own no anchor and its links open no excerpt, so a card's link to its own section shows that
 * section once and nothing nests. It holds its section's markup while it is open.
 */
export function Excerpt({ target }: { target: LinkTarget }) {
  const [read, setRead] = useState<ExcerptRead>(null);
  const name = target.anchor ?? "";
  const title = `${target.path}#${name}`;
  useEffect(() => {
    const reading = new AbortController();
    const url = `${api("", "/api/section", target.path.replace(/^\/+/, ""))}&format=blocks&anchor=${encodeURIComponent(name)}`;
    getJson(url, reading.signal).then(
      (value) => {
        if (reading.signal.aborted) return;
        const fragment = readFragment(value);
        setRead(
          fragment === null ? { error: "The server's section could not be read" } : { fragment },
        );
      },
      (error: unknown) => {
        if (!reading.signal.aborted) setRead({ error: said(error) });
      },
    );
    return () => reading.abort();
  }, [target.path, name]);
  return (
    // biome-ignore lint/a11y/useSemanticElements: a region of its own, as the Dioxus page draws it
    <div className="excerpt-box" role="region" aria-label={`Excerpt of ${title}`} data-busy="true">
      <p className="excerpt-title">{title}</p>
      <ExcerptBody read={read} />
    </div>
  );
}

/** An excerpt's read: its section, or why it could not be read; null until it answers. */
export type ExcerptRead = { fragment: Fragment } | { error: string } | null;

/**
 * What an excerpt's box shows of its read. Its blocks are drawn whole at once (`AtOnce`): opening
 * the excerpt asked for them, and the box, which scrolls, would clip their look-ahead, so a box
 * opened below the screen would stay empty until it was scrolled to.
 */
export function ExcerptBody({ read }: { read: ExcerptRead }) {
  if (read === null) return <p role="status">Loading the excerpt…</p>;
  if ("error" in read) return <p role="alert">{read.error}</p>;
  if (read.fragment.blocks.length === 0) return <p>This section is empty.</p>;
  return (
    <AtOnce.Provider value={true}>
      <HistoricalBlocks blocks={read.fragment.blocks} />
    </AtOnce.Provider>
  );
}

/** A document's commits, read when the reader opens them, and the diff of the one they pick. */
export function Versions({ path, scope }: { path: string; scope: string }) {
  const [versions, setVersions] = useState<Version[] | null>(null);
  const [diff, setDiff] = useState<DocumentDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mount = useMount();
  const read = async <T,>(
    url: string,
    parse: (value: unknown) => T | null,
    done: (read: T) => void,
  ) => {
    try {
      const value = await getJson(url, mount);
      if (mount.aborted) return;
      const parsed = parse(value);
      if (parsed === null) throw new Error("The server's answer could not be read");
      done(parsed);
    } catch (failure) {
      if (!mount.aborted) setError(said(failure));
    }
  };
  return (
    <details className="versions">
      {/* biome-ignore lint/a11y/noStaticElementInteractions: a summary is its fold's control, whose press (a click or a key) reads the versions, as the Dioxus page's did */}
      <summary
        onClick={() => void read(api(scope, "/api/versions", path), readVersions, setVersions)}
      >
        Versions and diffs
      </summary>
      {versions !== null && (
        <ul>
          {versions.map((version) => (
            <NearViewport key={version.sha} height={36}>
              <li>
                <Button
                  type="button"
                  onClick={() =>
                    void read(
                      `${api(scope, "/api/diff", path)}&sha=${encodeURIComponent(version.sha)}`,
                      readDiff,
                      setDiff,
                    )
                  }
                >
                  {`${version.date} · ${version.subject}`}
                </Button>
              </li>
            </NearViewport>
          ))}
        </ul>
      )}
      {diff !== null && (
        <>
          <h3>{diff.subject}</h3>
          <pre className="diff">{diff.diff}</pre>
        </>
      )}
      {error !== null && <p role="alert">{error}</p>}
    </details>
  );
}

/** The two notices' device copies as one: the oldest kept, unreachable if either is. */
export function bothShown(
  folder: [number, boolean] | null,
  intro: [number, boolean] | null,
): [number, boolean] | null {
  if (folder !== null && intro !== null) {
    return [Math.min(folder[0], intro[0]), folder[1] || intro[1]];
  }
  return folder ?? intro;
}

/** What a folder with no document says in their place: what makes the first one, and a card. */
export function FirstDocument({ folder }: { folder: Folder }) {
  if (folder.intro !== null || folder.entries.length > 0) return null;
  return (
    <FirstSteps>
      No document is here yet. A Markdown file in this folder is a page of its own, and a card in
      its register waits for your verdict in the queue:
    </FirstSteps>
  );
}

/** A folder's page: its head, its intro (a document drawn in it) and its entries. */
export function FolderPage({ path, scope }: { path: string; scope: string }) {
  // An entrypoint's own page, the main checkout's, is kept on this device, pinned, with its
  // intro's reads; no other folder page is.
  const [page] = useState(() => {
    const route = path.replace(/^\/+|\/+$/g, "");
    const kept =
      scope === "" && entrypoints(project()).includes(route)
        ? documentItem(`${route}/`, true)
        : null;
    return new FolderLive(path, scope, kept);
  });
  useSyncExternalStore(page.subscribe, page.snapshot, page.snapshot);
  useEffect(() => {
    const reading = new AbortController();
    void followFolder(page, reading.signal);
    return () => reading.abort();
  }, [page]);
  const [intro, setIntro] = useState<IntroNotice>(QUIET);
  const told = useCallback((notice: IntroNotice) => {
    setIntro((now) => (JSON.stringify(now) === JSON.stringify(notice) ? now : notice));
  }, []);
  const folder = page.folder;
  const noIntro = folder !== null && folder.intro === null;
  useEffect(() => {
    if (noIntro) setIntro(QUIET);
  }, [noIntro]);
  const device = bothShown(shown(page.freshness), intro.device);
  const retry = () => {
    page.retry.current += 1;
  };
  let body: ReactNode;
  if (folder !== null) {
    body = (
      <>
        <div className="doc-head">
          <p className="eyebrow">
            {unscoped(folder.path, scope)}
            {"/"}
          </p>
          <h1>{folder.title}</h1>
          <p className="doc-meta">
            <span className="pill">{folderPill(folder)}</span>
          </p>
        </div>
        {folder.intro !== null && (
          <DocumentPage key={folder.intro.path} path={folder.intro.path} scope={scope} embedded />
        )}
        {/* The page counts its documents once, in the pill above: the entries' heading gives no
            second number to disagree with it. */}
        <FirstDocument folder={folder} />
        {folder.entries.length > 0 && <h2 className="section-title">In this folder</h2>}
        <div className="folder-entries">
          {folder.entries.map((entry) => {
            const isFolder = entry.kind === "folder";
            return (
              <NearViewport key={entry.path} height={120}>
                <a
                  className={isFolder ? "card folder-entry tree-folder" : "card folder-entry"}
                  href={href(isFolder ? `/${entry.path}/` : `/${entry.path}`)}
                >
                  <strong className="tree-name">
                    {isFolder && (
                      <span className="tree-icon" aria-hidden="true">
                        ▸
                      </span>
                    )}
                    {entry.title}
                  </strong>
                  <div className="tree-pills">
                    {isFolder && (
                      <span className="pill">
                        {entry.docs === 1 ? "1 doc" : `${entry.docs} docs`}
                      </span>
                    )}
                    {entry.waiting > 0 && (
                      <span className="pill wait">{`${entry.waiting} waiting`}</span>
                    )}
                  </div>
                  {entry.summary !== "" && <p>{entry.summary}</p>}
                </a>
              </NearViewport>
            );
          })}
        </div>
      </>
    );
  } else if (page.missing) {
    body = (
      <Trouble
        title="Folder not found"
        text={`There is no folder at ${path}: it may have been moved, renamed or removed.`}
        back={[scoped(scope, documentsHref(project())), "All documents"]}
        retry={retry}
      />
    );
  } else if (page.error !== null) {
    body = (
      <Trouble
        title="The folder could not be loaded"
        text={failedRead(page.error, true)}
        retry={retry}
      />
    );
  } else {
    body = <p role="status">Loading folder…</p>;
  }
  return (
    <FolderNoticeContext.Provider value={told}>
      <KeptFolderContext.Provider value={page.kept}>
        <div
          className="document-page"
          data-ready={folder !== null ? "folder" : readyWord(page.missing, page.error !== null)}
          data-feed={intro.stale ? "stale" : "live"}
          data-cache={device ? "device" : undefined}
        >
          {body}
          <PageNotices
            reconnecting={intro.stale}
            reconnect={RECONNECT}
            error={folder !== null ? (page.error ?? intro.error) : ""}
            documentRemoved={intro.removed}
            device={device}
          />
        </div>
      </KeptFolderContext.Provider>
    </FolderNoticeContext.Provider>
  );
}

/**
 * The sandbox an HTML document's frame draws it in: its scripts run, in an opaque origin, never the
 * app's, so the app's storage, cookies, feed and API are out of its reach; no same-origin, top
 * navigation, forms, popups or dialogs. Never add `allow-same-origin`.
 */
export const SANDBOX = "allow-scripts";

/**
 * An HTML document: the file drawn whole, as its author made it, in a sandboxed frame inside the
 * app's page, under the app's header: its path, as a document's head shows it, over its frame, as
 * wide as the page and as tall as the window below the header, scrolling inside itself. None of the
 * Markdown view's features: no outline, cards, notes, asks, editing, versions, device cache or
 * feed (docs/operations/PROJECT_FILE.md, "HTML documents"). The frame loads the file's own address, the route
 * with the file's query (`fileAddress`), which the server answers with the file itself under a
 * policy that keeps it to its entrypoint's files and opens no connection
 * (`apps/symphony-server/src/media_files.rs`, `html_document`).
 */
export function HtmlPage({ path, scope }: { path: string; scope: string }) {
  const src = fileAddress(href(scoped(scope, `/${path}`)));
  const name = path.split("/").at(-1) ?? path;
  return (
    <div className="document-page html-document" data-ready="html">
      <p className="eyebrow">{path}</p>
      <iframe
        className="html-frame"
        src={src}
        sandbox={SANDBOX}
        title={name}
        referrerPolicy="no-referrer"
      />
    </div>
  );
}
