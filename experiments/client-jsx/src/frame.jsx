// The frame every page is drawn in (`frame.rs`): the header's pages and crumbs, the Appearance
// and Activity buttons, the dock and the `+` button. It reads `/api/activity` and `/api/pending`
// as the Dioxus frame does on every page. Their panels (the persistent chat, activity and
// appearance module) are not part of this experiment: the buttons draw, and open nothing.
import { useEffect, useState } from "react";
import { get } from "./api.js";
import { documentsHref, href, project } from "./project.js";

export const FEED_NOTICE = "Reconnecting: the board may be out of date";

function ownPages() {
  return [
    ["/", "Home"],
    ["/queue", "Queue"],
    [documentsHref(), "Docs"],
    ["/tasks", "Board"],
    ["/reports", "Reports"],
    ["/agents", "Agents"],
  ];
}

/** The header's links for the route `path` (no leading slash) and whether it is a document. */
function headerLinks(path, document) {
  const active = [
    path === "" && !document,
    path === "queue",
    document,
    path === "tasks" || path.startsWith("task/"),
    path === "reports",
    path === "agents",
  ];
  return ownPages()
    .map(([to, label], index) => ({ href: to, label, active: active[index] }))
    .concat(project.nav.map((item) => ({ href: item.href, label: item.label, active: item.document === path })));
}

/** The crumbs after the lit link: a task's slug under Board, a document's folders and name. */
function crumbs(path, document) {
  if (path.startsWith("task/")) return [path.slice("task/".length)];
  if (!document) return [];
  const parts = path.split("/").filter(Boolean);
  return parts.length > 1 ? parts : [];
}

export function Frame({ route, children }) {
  const [pending, setPending] = useState(null);
  useEffect(() => {
    let current = true;
    Promise.all([get("/api/activity"), get("/api/pending")])
      .then(([, waiting]) => current && setPending(waiting))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [route.path]);
  const path = route.path.replace(/^\//, "");
  const links = headerLinks(path, route.document);
  const trail = crumbs(path, route.document);
  return (
    <div className="app-frame">
      <header className="app-header" id="top">
        {project.base ? (
          <a href="/" data-native="true" className="project-switch">
            Projects
          </a>
        ) : null}
        <nav className="app-pages" aria-label="Pages">
          {links.map((link) => (
            <a key={link.href} href={href(link.href)} className={link.active ? "active" : ""} aria-current={link.active ? "page" : undefined}>
              {link.label}
            </a>
          ))}
        </nav>
        {trail.length ? (
          <nav className="crumbs" aria-label="Breadcrumbs">
            {trail.map((crumb, index) => (
              <span key={index}>
                {index > 0 ? " / " : ""}
                {index + 1 < trail.length ? <a href={href(`/${trail.slice(0, index + 1).join("/")}/`)}>{crumb}</a> : <span>{crumb}</span>}
              </span>
            ))}
          </nav>
        ) : null}
        <button id="appearance" type="button" className="dx-button" data-style="outline" data-size="default" aria-label="Appearance" title="Appearance" aria-expanded="false" aria-controls="appearance-panel">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
            <circle cx="12" cy="12" r="9" />
            <path d="M12 3a9 9 0 0 1 0 18z" fill="currentColor" />
          </svg>
        </button>
        <button id="bell" type="button" className="dx-button" data-style="outline" data-size="default" aria-expanded="false" aria-controls="activity-panel">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
            <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
          </svg>{" "}
          Activity
        </button>
      </header>
      <div className="frame-page">{children}</div>
      <div className="frame-dock" aria-hidden="true" />
      {pending?.items?.length ? (
        <nav className="pending-nav frame-pending" aria-label="Cards waiting on you">
          <button type="button" className="dx-button" data-style="outline" data-size="default" data-pending-step="prev" title="Previous pending decision ([)">
            ← prev
          </button>
          <button type="button" className="dx-button" data-style="outline" data-size="default" data-pending-step="next" title="Next pending decision (])">
            next →
          </button>
          <a href="#top">↑ top</a>
        </nav>
      ) : null}
      <button id="quick-open" type="button" className="dx-button" data-style="outline" data-size="default" aria-label="Chat or add a task" aria-controls="quick-sheet" aria-expanded="false">
        +
      </button>
    </div>
  );
}

/** `live::PageNotices`: what the page waits on, and the room it keeps for it. */
export function PageNotices({ reconnecting, error = "", reconnect = FEED_NOTICE }) {
  const visible = reconnecting || !!error;
  return (
    <>
      <div className="stale page-notices notice-stack" hidden={!visible}>
        {reconnecting ? (
          <p className="pill wait" role="status">
            {reconnect}
          </p>
        ) : null}
        {error ? (
          <p className="pill wait" role="alert">
            {error}
          </p>
        ) : null}
      </div>
      <div className="feed-notice-space page-notice-reserve notice-stack" aria-hidden="true">
        <p className="pill">{reconnect}</p>
        {error ? <p className="pill">{error}</p> : null}
      </div>
    </>
  );
}

/** A performance mark, once per name (the Dioxus pages mark `tasks-ready` and `document-ready`). */
const marked = new Set();
export function mark(name) {
  if (marked.has(name)) return;
  marked.add(name);
  performance.mark(name);
}
