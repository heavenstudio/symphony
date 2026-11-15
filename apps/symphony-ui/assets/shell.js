// One history adapter; switching page kinds awaits the old runtime's cleanup.
const modules = __MODULES__;
try {
    const css = localStorage.getItem(stored("studio.cssVars"));
    if (css) {
        const style = document.createElement("style");
        style.id = "studio-theme";
        style.textContent = css;
        document.head.append(style);
        document.documentElement.setAttribute("data-studio-theme", "");
    }
} catch (_) { /* Storage can be unavailable in a private browser. */ }
let active;
let requested = 0;
let switching = Promise.resolve();
const styleLoads = new Map();
const failed = new Set();
const preloaded = new Set();
// The page's own reads in flight (its modules' wasm, its data) and when the last one answered:
// the idle warm-up waits until none has been in flight for half a second, and after a read that
// got no answer (its time ran out, the link dropped), until one does.
let reads = 0, answered = 0;
const read = window.fetch;
window.fetch = (...args) => (reads++, read(...args).then(
    response => (answered = Date.now(), response),
    error => { answered = Infinity; throw error; },
).finally(() => reads--));

// The modules every page kind shares, each loaded once on its first use and kept mounted: the
// chat and activity panel, and the overlay host that draws a select's popup or the picture
// viewer for any module (components/deferred.rs). A select never waits on the chat's module.
// Each has its root, its ready flag (`reviewPanelReady`, `reviewOverlayReady`) and event
// (`review:panel-ready`...), and its failure notice (`#panel-error`...) with a retry.
const PERSISTENT = {
    panel: { root: "quick-root", ready: "reviewPanelReady", said: "Conversation could not be loaded. ", retry: "Retry loading conversation" },
    overlay: { root: "overlay-root", ready: "reviewOverlayReady", said: "Menus and pictures could not be loaded. ", retry: "Retry loading menus" },
};
const persistent = new Map();

// A failed load's alert `id` in the notice region: what failed, and a button that takes the
// alert down and tries `again`.
function alarm(id, said, retry, again) {
    let notice = document.getElementById(id);
    if (!notice) {
        notice = document.createElement("div");
        notice.id = id;
        notice.className = "shell-notice";
        notice.setAttribute("role", "alert");
        document.getElementById("shell-notices").append(notice);
    }
    const button = document.createElement("button");
    button.textContent = retry;
    button.onclick = () => { notice.remove(); again(); };
    notice.replaceChildren(said, button);
}

// Whether this page's build is gone, and the page is loading again onto the one the server serves
// now. A published client keeps a build two publications more (KEEP in
// crates/symphony/src/cli/publish.rs), so a page open longer asks for a module the server no
// longer has: its import fails. The page then asks for its own shell, bypassing the cache, and a
// 404 says the build is gone; it loads again, once per build it was opened on (kept in the tab's
// storage, without which it never does), and otherwise says the failure as before.
async function gone() {
    const own = new URL(import.meta.url).pathname;
    try {
        if ((await read(own, { cache: "no-store" })).status !== 404) return false;
        const key = stored("symphony-ui-gone-build");
        if (sessionStorage.getItem(key) === own) return false;
        sessionStorage.setItem(key, own);
    } catch { return false; }
    location.reload();
    return true;
}

// The two the program's chunk loader (assets/chunks.js) says a failed load of its media layer with:
// what the shell's own failures use, so every page's first visit carries none of the program's.
window.reviewShell = { gone, alarm };

// Each of `hrefs` asked for once, by a link of `attributes`, beside the module that needs it.
function preload(hrefs, attributes) {
    for (const href of hrefs) {
        if (href && !preloaded.has(href)) {
            preloaded.add(href);
            document.head.insertAdjacentHTML("beforeend", `<link ${attributes} href="${href}">`);
        }
    }
}

async function initialize(kind) {
    const spec = modules[kind];
    // A kind of the web client (apps/symphony-web) is its script, which exports its own `run` and
    // `stop`: no wasm, no initializer to wait on. The script asks for the chunks it imports only
    // once it is in: a module preload asks for them beside it.
    if (!spec.wasm) {
        preload(spec.imports, 'rel="modulepreload"');
        return import(spec.script);
    }
    // The module's initializer fetches its wasm only once its script has been fetched and run, and
    // a program kind's run its chunk (src/split.rs) only once that wasm has: a preload asks for all
    // at once, and each fetch takes the preloaded response.
    preload([spec.wasm, spec.chunk], 'rel="preload" as="fetch" crossorigin');
    const module = await import(spec.script);
    try {
        if (failed.has(kind)) await module.default({ module_or_path: spec.wasm });
        else await window[`reviewModuleReady_${kind}`];
        failed.delete(kind);
    } catch (error) { failed.add(kind); throw error; }
    // A program kind draws nothing before its chunk is in: awaited here, the page shown before stays
    // until then, and a chunk that does not load rejects while it still shows (chunks.js forgets the
    // failure, so a retry fetches it again). The kind's `run` then finds the chunk in.
    if (spec.chunk) await window.reviewChunk(kind);
    return window[`reviewModule_${kind}`];
}

// Every module's Dioxus hears a bubbling event at its root and gives it to the element of its own
// that bears the number in the `data-dioxus-id` nearest the event's target; each module counts
// those numbers on its own. A root inside another's would have its events taken by the outer module
// too, for whatever element of its own bears the same number. So what listens on a module's root
// hears only the events of that module's own part of the page, the ones whose nearest module root
// on their path is that root.
// Nothing is stopped: the inner module hears the event at its own root, and the document and the
// window after, as before.
const moduleRoots = new WeakSet();
function scope(mount) {
    moduleRoots.add(mount);
    const add = mount.addEventListener, remove = mount.removeEventListener, scoped = new WeakMap();
    const own = event => event.composedPath().find(node => moduleRoots.has(node)) === mount;
    mount.addEventListener = function (type, listener, options) {
        if (!listener) return add.call(this, type, listener, options);
        let heard = scoped.get(listener);
        if (!heard) {
            heard = function (event) {
                if (!own(event)) return;
                return typeof listener === "function" ? listener.call(this, event) : listener.handleEvent(event);
            };
            scoped.set(listener, heard);
        }
        return add.call(this, type, heard, options);
    };
    mount.removeEventListener = function (type, listener, options) {
        return remove.call(this, type, (listener && scoped.get(listener)) || listener, options);
    };
}

function mountFor(kind) {
    const shared = PERSISTENT[kind];
    const id = shared ? shared.root : `review-root-${kind}`;
    let mount = document.getElementById(id);
    if (!mount) {
        mount = document.createElement("div");
        mount.id = id;
        mount.hidden = !shared;
        (shared ? document.body : document.getElementById("main")).append(mount);
    }
    if (!moduleRoots.has(mount)) scope(mount);
    return mount;
}

function styles(spec) {
    const pending = [];
    for (const href of spec.styles) {
        let loaded = styleLoads.get(href);
        if (!loaded) {
            let link = [...document.querySelectorAll("link[rel=stylesheet]")].find(link => link.getAttribute("href") === href);
            if (link?.sheet) loaded = Promise.resolve();
            else {
                const fresh = !link;
                link ||= document.createElement("link");
                loaded = new Promise((resolve, reject) => {
                    link.addEventListener("load", resolve, { once: true });
                    link.addEventListener("error", () => {
                        styleLoads.delete(href);
                        link.remove();
                        reject(new Error(`Styles could not load: ${href}`));
                    }, { once: true });
                });
                // Page styles go before the project's theme package, which wins the cascade.
                if (fresh) { link.rel = "stylesheet"; link.href = href; document.head.insertBefore(link, document.head.querySelector("[data-symphony-theme]")); }
            }
            styleLoads.set(href, loaded);
        }
        pending.push(loaded);
    }
    return Promise.all(pending);
}

// A persistent module's script, wasm and styles are requested together; it mounts once all three
// are in. A failure says so with a retry, and the next use tries again. `focus`: a click opened
// the panel's sheet, which takes the focus.
function ensure(kind, focus) {
    const shared = PERSISTENT[kind];
    const failure = `${kind}-error`;
    if (!persistent.has(kind)) {
        persistent.set(kind, (async () => {
            mountFor(kind);
            const [imported] = await Promise.all([initialize(kind), styles(modules[kind])]);
            await imported.run();
            document.getElementById(failure)?.remove();
            window[shared.ready] = true;
            dispatchEvent(new Event(`review:${kind}-ready`));
            return { imported };
        })().catch(async error => {
            persistent.delete(kind);
            if (await gone()) return;
            console.error(shared.said, error);
            if (kind === "panel") loadingSheets();
            alarm(failure, shared.said, shared.retry, () => ensure(kind));
        }));
    }
    if (kind === "panel") loadingSheets(focus);
    return persistent.get(kind);
}

// Until the panel's module mounts, the sheet or activity list a click opened is drawn here at
// once, with the page frame's own styles and a status line, never a blank gutter. A click's sheet
// takes the focus (`+` hides at desktop width), and keeps it when redrawn; Escape closes it and
// gives the focus back to its opener (`reviewPanelOpener`, as the frame recorded it). Its status
// is filled a frame after the sheet is in the page, so a screen reader announces it. The real
// panel replaces it and takes the focus into its own sheet (frame_panel.rs `take_focus`); a
// failed load takes it down beside the retry notice. (A spinner's markup alone would cost every
// first visit some 400 gzip bytes of the owner's budget.)
function loadingSheets(focus) {
    const old = document.getElementById("panel-loading");
    focus ||= old?.contains(document.activeElement);
    old?.remove();
    if (!persistent.has("panel") || window.reviewPanelReady) return;
    const sheet = (open, id, label, inner, what) => open ? `<aside id="${id}" aria-label="${label}" tabindex="-1">${inner}<p role="status" data-what="${what}"></p></aside>` : "";
    const sheets = sheet(window.reviewActivityOpen === true, "activity-panel", "Recent activity", `<div class="panel-heading"><h2>Recent activity</h2></div>`, "activity")
        + sheet(window.reviewQuickDraft?.open === true, "quick-sheet", "Chat or add a task", "", "conversation");
    if (!sheets) return;
    document.body.insertAdjacentHTML("beforeend", `<div id="panel-loading">${sheets}</div>`);
    if (focus) document.querySelector("#panel-loading aside").focus();
    requestAnimationFrame(() => requestAnimationFrame(() => {
        for (const status of document.querySelectorAll("#panel-loading [data-what]")) status.textContent = `Loading ${status.dataset.what}…`;
    }));
}
addEventListener("review:panel-ready", () => document.getElementById("panel-loading")?.remove());
addEventListener("keydown", event => {
    const sheet = event.key === "Escape" && event.target instanceof Element && event.target.closest("#panel-loading aside");
    if (!sheet) return;
    const quick = sheet.id === "quick-sheet", opener = window.reviewPanelOpener;
    window.reviewPanelOpener = window.reviewPanelFocus = null;
    if (quick) window.reviewQuickDraft.open = false; else window.reviewActivityOpen = false;
    dispatchEvent(new Event(quick ? "review:quick" : "review:activity"));
    requestAnimationFrame(() => (opener?.isConnected ? opener : document.getElementById(quick ? "quick-open" : "bell"))?.focus());
});

// Selects' popups and the picture viewer are data-only requests to the overlay module's host.
window.reviewOverlay = request => {
    if (request.kind === "close") {
        if (window.reviewOverlayRequest?.owner !== request.owner) return;
        window.reviewOverlayRequest = null;
        dispatchEvent(new Event("review:overlay"));
        return;
    }
    window.reviewOverlayRequest = request;
    ensure("overlay").then(() => dispatchEvent(new Event("review:overlay")));
};

// A live update about to be drawn keeps what the reader looks at in place: the focused field on
// the screen, else the first of `rows` on it. Only where the reader has scrolled: the box that
// scrolls around it once it is off its top, else the page once it is. One at its top shows
// what lands at the top. Each frame the box and then the page scroll by however far it moved, until
// four pass with nothing moved and no reserved box waiting to draw within its sentinel's reach:
// 600 px above and below what its own scroller shows of the window (`.viewport-sentinel`, as
// `viewport::waits_near` decides), for a box past it never draws. However many frames that takes, as
// on a slow phone, and 1800 at most (half a minute at sixty a second; a hidden tab draws none), a
// backstop only a box that never draws reaches. A move the box and the page cannot take back (one
// at its end) counts as nothing moved once it stays. The reader's own scroll, click or key (typing
// in a field aside) ends the hold.
window.reviewHoldPlace = rows => {
    const scrollerOf = node => {
        while ((node = node.parentElement) && !(node.scrollHeight > node.clientHeight && /auto|scroll/.test(getComputedStyle(node).overflowY)));
        return node;
    };
    const shows = node => {
        const top = node.getBoundingClientRect().top, box = scrollerOf(node)?.getBoundingClientRect();
        return top >= 0 && top < innerHeight && (!box || (top >= box.top && top < box.bottom));
    };
    const field = document.activeElement;
    const anchor = field?.matches("input, textarea, select") && shows(field) ? field : [...document.querySelectorAll(rows)].find(shows);
    if (!anchor) return;
    const scroller = scrollerOf(anchor), page = scrollY > 0;
    if (scroller ? scroller.scrollTop <= 0 : !page) return;
    const top = anchor.getBoundingClientRect().top, gestures = ["wheel", "touchstart", "mousedown", "keydown"];
    let stopped = false, frame = 0, quiet = 0, stuck = 0;
    const gesture = event => { if (event.type !== "keydown" || !event.target.matches?.("input, textarea, select, [contenteditable]")) stopped = true; };
    // Whether `r` meets the window, cut to `clip` (a scroller's box) when there is one, and 600 px
    // above and below it.
    const reaches = (r, clip) => r.bottom > r.top && r.right > r.left
        && r.bottom > Math.max(0, clip?.top ?? 0) - 600 && r.top < Math.min(innerHeight, clip?.bottom ?? innerHeight) + 600
        && r.right > Math.max(0, clip?.left ?? 0) && r.left < Math.min(innerWidth, clip?.right ?? innerWidth);
    const near = () => [...document.querySelectorAll("[data-pending=true]")].some(box => {
        const r = box.getBoundingClientRect();
        return reaches(r) && reaches(r, scrollerOf(box)?.getBoundingClientRect());
    });
    const hold = () => {
        if (!stopped && anchor.isConnected) {
            const moved = anchor.getBoundingClientRect().top - top;
            let left = moved;
            if (Math.abs(left) >= 0.5 && scroller) {
                const before = scroller.scrollTop;
                scroller.scrollTop = before + Math.round(left);
                left -= scroller.scrollTop - before;
            }
            if (Math.abs(left) >= 0.5 && page) {
                const before = scrollY;
                scrollBy(0, left);
                left -= scrollY - before;
            }
            quiet = Math.abs(moved - stuck) >= 0.5 || near() ? 0 : quiet + 1;
            stuck = left;
            if (quiet < 4 && ++frame < 1800) return requestAnimationFrame(hold);
        }
        gestures.forEach(kind => removeEventListener(kind, gesture, true));
    };
    gestures.forEach(kind => addEventListener(kind, gesture, true));
    requestAnimationFrame(hold);
};

// `then`, once the page has drawn its data (none of its reads in flight for half a second, the
// last of them answered) and the browser is idle.
function settled(then) {
    (window.requestIdleCallback || setTimeout)(function next() {
        if (reads || Date.now() - answered < 500) return setTimeout(next, 500);
        then();
    }, { timeout: 5000 });
}

// A task page's timeline box loads the timeline's chunk once the page has drawn its data and the
// browser is idle (apps/symphony-web/src/timeline-slot.tsx), as a persistent module's files are
// warmed: the page's own reads first.
window.reviewSettled = settled;

// Once the first page has drawn its data (none of its reads in flight for half a second, the last
// of them answered) and the
// browser is idle, the persistent modules' files go into the HTTP cache at the lowest priority,
// nothing evaluated, so a first open waits on no network: the chat's first (the program's script
// and main module, which /agents has in already, and its chunk), then, once its chunk (or its
// own load, if a click came first) is in, the menus', so no two share a slow link. A browser asking
// to save data, or a 2g link, skips it; the first open then fetches them. A browser without idle
// callbacks warms them at once; one without prefetch (Safari) ignores it.
let warmed = false;
function warm() {
    const link = navigator.connection, kinds = Object.keys(PERSISTENT);
    if (warmed || link?.saveData || /2g/.test(link?.effectiveType)) return;
    warmed = true;
    (window.requestIdleCallback || setTimeout)(function next() {
        if (reads || Date.now() - answered < 500) return setTimeout(next, 500);
        const kind = kinds.shift();
        if (!kind) return;
        if (persistent.has(kind)) return persistent.get(kind).then(next);
        const { script, wasm, styles, chunk } = modules[kind];
        for (const href of [wasm, script, chunk, ...styles]) {
            if (!document.querySelector(`link[href="${href}"]`)) document.head.insertAdjacentHTML("beforeend", `<link rel="prefetch" href="${href}">`);
        }
        const last = document.querySelector(`link[rel=prefetch][href="${chunk}"]`);
        if (last) last.onload = last.onerror = next; else next();
    }, { timeout: 5000 });
}

// Read ahead (docs/analysis/client-cache.md, "Read ahead"): once a page has drawn its data, its
// feed is open and the browser is idle, the first AHEAD task and document links it shows on the
// screen are read into the client cache one after another, at the lowest priority and outside the
// page's own reads (cache.js, `ahead`), so opening one draws at once: a task's page and its
// thread's newest page, a document's first part (`view=first`, at most
// symphony_api::AHEAD_BYTES). Not when the browser asks to save data or the link is 3g or slower;
// never a link that acts (a download, `data-native`, another window), a search, a folder,
// another checkout's page or the page itself. A page whose feed is not open within a minute
// reads nothing ahead.
const AHEAD = 8;
function ahead(generation, tries = 60) {
    const link = navigator.connection;
    if (link?.saveData || /2g|3g/.test(link?.effectiveType) || !window.symphonyCache) return;
    settled(() => {
        if (generation !== requested) return;
        const feed = document.querySelector("main[data-feed], main [data-feed]")?.dataset.feed;
        if (feed !== "live") return feed && tries > 1 && setTimeout(() => ahead(generation, tries - 1), 1000);
        const base = project().base || "", id = base.replace(/^\/+/, ""), items = new Map();
        for (const anchor of document.querySelectorAll("main a[href]")) {
            const box = anchor.getBoundingClientRect(), url = inPlace(anchor);
            if (!url || url.search || url.pathname === location.pathname
                || box.bottom <= 0 || box.top >= innerHeight || box.right <= 0 || box.left >= innerWidth || !box.width) continue;
            const kind = route(url.pathname);
            let path;
            try { path = decodeURIComponent(url.pathname).slice(base.length).replace(/\/{2,}/g, "/").replace(/^\/(app\/)?/, ""); } catch { continue; }
            if (path.startsWith("@")) continue;
            path = path.replace(/^doc\//, "");
            const api = name => `${base}/api/${name}`, encoded = encodeURIComponent;
            if (kind === "task" && /^task\/./.test(path)) {
                const slug = path.slice(5).replace(/\/$/, "");
                items.set(`task:${slug}`, ["task", [["page", api(`task-page?slug=${encoded(slug)}`)], ["thread", api(`task-history?slug=${encoded(slug)}`)]]]);
            } else if (kind === "docs" && path.endsWith(".md")) {
                items.set(`document:${path}`, ["document", [["outline", api(`document?path=${encoded(path)}&view=first`)]]]);
            }
            if (items.size === AHEAD) break;
        }
        (async () => {
            for (const [item, [kind, reads]] of items) {
                if (generation !== requested) return;
                await symphonyCache.ahead(read, id, item, kind, reads);
            }
        })();
    });
}

// The routes the Symphony answers itself, symphony_api::SYMPHONY_ROUTES word for word (a test
// keeps them equal): they win over every entrypoint.
const SYMPHONY_ROUTES = ["tasks", "task", "agents", "queue", "reports", "api", "assets", "static", "thumb", "x", "artifacts/uploads", "app", "doc", "history", "diff", "wasm", "_dioxus", "favicon.ico"];

function within(route, path) {
    return route === "" || path === route || path.startsWith(`${route}/`);
}

// The project the server inlines in the page (symphony_api::PROJECT_ELEMENT): its plugins'
// pages and its entrypoints. A page without it is the plain project.
function project() {
    try { return JSON.parse(document.getElementById("symphony-project")?.textContent || "{}"); } catch { return {}; }
}

// `key` as the project keeps it in the browser (project::stored): the one server's projects share
// its origin, so each key takes the project's id first; a server of its own keeps it as it is.
function stored(key) {
    const id = (project().base || "").replace(/^\/+/, "");
    return id ? `${id}:${key}` : key;
}

// Whether `path` lies under the project's base (ProjectView::base): the one server serves each
// project at `/<id>/`, a server of its own at `/`. Every link the client draws is written there
// (project::href), so a click follows its address as a new tab or a copy of it does; a link
// elsewhere, the project switch's, loads as the browser loads it.
function ours(path) {
    const base = project().base || "";
    return !base || path === base || path.startsWith(`${base}/`);
}

function route(path) {
    try { path = decodeURIComponent(path); } catch { return null; }
    const base = project().base || "";
    if (base && (path === base || path.startsWith(`${base}/`))) path = path.slice(base.length) || "/";
    // Keep module selection aligned with navigation.rs for scoped URLs and repeated slashes.
    // The empty task route below uses the board module.
    path = path.replace(/\/{2,}/g, "/").replace(/^\//, "").replace(/^app\//, "");
    if (path.startsWith("@")) {
        const slash = path.indexOf("/");
        if (slash < 0) return null;
        path = path.slice(slash + 1).replace(/^app\//, "");
    }
    path = path.replace(/^doc\//, "");
    const clean = path.replace(/\/$/, "");
    if (clean === "tasks" || clean === "task") return "board";
    if (path.startsWith("task/")) return "task";
    if (clean === "agents") return "tasks";
    if (clean === "queue" || clean === "reports") return clean;
    const view = project();
    const pages = (view.nav || []).map(item => item.href.replace(/^\/+|\/+$/g, "")).filter(Boolean);
    if (pages.includes(clean)) return "docs";
    // A document or folder page of an entrypoint (ProjectView::is_document_route): a trailing
    // slash or an entrypoint's own route names a folder, whatever its name holds; an HTML page (a
    // name before `.html` or `.htm`: `.html` alone names none, as symphony_api::is_html) is a
    // document, its frame drawn in the documents' kind.
    const urls = view.entrypoints?.length ? view.entrypoints : ["docs"];
    const name = clean.split("/").pop();
    if (!SYMPHONY_ROUTES.concat(pages).some(taken => within(taken, clean))
        && urls.some(url => within(url, clean))
        && (path.endsWith("/") || urls.includes(clean) || !name.includes(".") || name.endsWith(".md")
            || /.\.html?$/i.test(name))) return "docs";
    return clean === "" ? "review" : null;
}

function scrollAnchor(preserveScroll) {
    let id;
    try { id = decodeURIComponent(location.hash.slice(1)); } catch { return; }
    if (!id) { if (!preserveScroll) scrollTo(0, 0); return; }
    const target = document.getElementById(id);
    if (!target) return;
    for (let parent = target; parent; parent = parent.parentElement) {
        if (parent instanceof HTMLDetailsElement) parent.open = true;
    }
    target.scrollIntoView();
}

async function show(generation, scroll) {
    if (generation !== requested) return;
    const kind = route(location.pathname);
    if (!kind) { location.reload(); return; }
    if (active?.kind !== kind) {
        const spec = modules[kind];
        // Keep the previous page while its replacement loads (its script, wasm and any chunk), and
        // under the notice when it does not. Its styles are requested with its script and wasm;
        // insets and row geometry must apply before page effects can anchor.
        const mount = mountFor(kind);
        const [imported] = await Promise.all([initialize(kind), styles(spec)]);
        if (generation !== requested) return;
        if (active) {
            await active.imported.stop();
            active.mount.hidden = true;
        }
        mount.hidden = false;
        await imported.run();
        active = { kind, imported, mount };
    }
    document.getElementById("module-error")?.remove();
    const root = document.scrollingElement;
    const position = {
        x: scrollX, y: scrollY,
        maxX: Math.max(0, root.scrollWidth - root.clientWidth),
        maxY: Math.max(0, root.scrollHeight - root.clientHeight),
    };
    dispatchEvent(new Event("review:navigate"));
    requestAnimationFrame(() => requestAnimationFrame(() => {
        if (generation !== requested) return;
        // Only shrinking bounds after the mounted baseline can account for a later clamp.
        // Movement while switching away from the old page must not count as a new scroll.
        const maxX = Math.max(0, root.scrollWidth - root.clientWidth);
        const maxY = Math.max(0, root.scrollHeight - root.clientHeight);
        const x = maxX < position.maxX ? Math.min(position.x, maxX) : position.x;
        const y = maxY < position.maxY ? Math.min(position.y, maxY) : position.y;
        const moved = x !== scrollX || y !== scrollY;
        const restored = scroll.initial && (scroll.x !== 0 || scroll.y !== 0);
        scrollAnchor(moved || restored);
        document.documentElement.dataset.navigation = "ready";
        warm();
        ahead(generation);
    }));
}

function navigate() {
    const overlay = window.reviewOverlayRequest;
    if (overlay) {
        window.reviewOverlayRequest = null;
        dispatchEvent(new Event("review:overlay"));
        overlay.callback?.(null);
    }
    const generation = ++requested;
    const scroll = { x: scrollX, y: scrollY, initial: !active };
    document.documentElement.dataset.navigation = "loading";
    switching = switching.catch(() => {}).then(() => show(generation, scroll)).catch(async error => {
        if (await gone()) return;
        if (generation === requested) document.documentElement.dataset.navigation = "failed";
        console.error("Page could not be loaded", error);
        alarm("module-error", "This page could not be loaded. ", "Retry", navigate);
    });
}

// The address a click on `link` takes in place, the shell's to follow, or nothing: a link that
// acts (a download, `data-native`, another window) or leads off the project's pages is the
// browser's.
function inPlace(link) {
    if (!link || link.hasAttribute("download") || link.dataset.native === "true" || (link.target && link.target !== "_self")) return;
    const url = new URL(link.href, location.href);
    if (url.origin === location.origin && ours(url.pathname) && route(url.pathname)) return url;
}

document.addEventListener("click", event => {
    if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    const url = inPlace(event.target instanceof Element && event.target.closest("a[href]"));
    if (!url) return;
    event.preventDefault();
    if (document.querySelector("[data-posting=true]")) return;
    history.pushState(null, "", url);
    navigate();
});
addEventListener("popstate", navigate);
// Opening the "+" sheet or the activity list loads the panel; closing one while it loads takes
// its loading sheet down. The header's Appearance panel is in the same module and has no loading
// sheet: it takes the focus itself once the module mounts.
addEventListener("review:quick", () => { if (window.reviewQuickDraft?.open) ensure("panel", true); else loadingSheets(); });
addEventListener("review:activity", () => { if (window.reviewActivityOpen) ensure("panel", true); else loadingSheets(); });
addEventListener("review:appearance", () => { if (window.reviewAppearanceOpen) ensure("panel"); });
addEventListener("review:activity-arrived", () => ensure("panel"));
async function pendingStep(step) {
    await ensure("panel");
    requestAnimationFrame(() => requestAnimationFrame(() => {
        document.querySelector(`[data-pending-action=${step}]`)?.click();
    }));
}
document.addEventListener("click", event => {
    const step = event.target instanceof Element && event.target.closest("[data-pending-step]")?.dataset.pendingStep;
    if (step) { event.preventDefault(); pendingStep(step); }
});
addEventListener("keydown", event => {
    if (window.reviewPanelReady || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.target instanceof Element && event.target.closest("input,textarea,select,[contenteditable],dialog[open],[role=dialog],[role=listbox],[role=menu]")) return;
    const step = event.key === "[" ? "prev" : event.key === "]" ? "next" : null;
    if (step) { event.preventDefault(); pendingStep(step); }
});
addEventListener("beforeunload", event => {
    if (document.querySelector("[data-posting=true]")) { event.preventDefault(); event.returnValue = ""; }
});
navigate();
