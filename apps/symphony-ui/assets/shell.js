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
// (`review:panel-ready`...), and its failure notice (`#panel-error`...) with a retry. The task
// timeline's (task_timeline.rs) is one too, but cold: never warmed, it is loaded only for a task
// page that asks for it, and its root goes in that page's box.
const PERSISTENT = {
    panel: { root: "quick-root", ready: "reviewPanelReady", said: "Conversation could not be loaded. ", retry: "Retry loading conversation" },
    overlay: { root: "overlay-root", ready: "reviewOverlayReady", said: "Menus and pictures could not be loaded. ", retry: "Retry loading menus" },
    timeline: { root: "timeline-root", ready: "reviewTimelineReady", said: "The timeline could not be loaded. ", retry: "Retry loading the timeline", cold: true },
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

async function initialize(kind) {
    const spec = modules[kind];
    // The module's initializer fetches its wasm only once its script has been fetched and run.
    // A preload asks for both at once; the initializer's own fetch takes the preloaded response.
    if (!preloaded.has(spec.wasm)) {
        preloaded.add(spec.wasm);
        document.head.insertAdjacentHTML("beforeend", `<link rel="preload" as="fetch" crossorigin href="${spec.wasm}">`);
    }
    const module = await import(spec.script);
    try {
        if (failed.has(kind)) await module.default({ module_or_path: spec.wasm });
        else await window[`reviewModuleReady_${kind}`];
        failed.delete(kind);
        return window[`reviewModule_${kind}`];
    } catch (error) { failed.add(kind); throw error; }
}

// Every module's Dioxus hears a bubbling event at its root and gives it to the element of its own
// that bears the number in the `data-dioxus-id` nearest the event's target; each module counts
// those numbers on its own. A root inside another's (the timeline's, in a task page's box) would
// have its events taken by the outer module too, for whatever element of its own bears the same
// number (the timeline's Show all and the page's +). So what listens on a module's root hears only
// the events of that module's own part of the page, the ones whose nearest module root on their
// path is that root.
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
// what lands at the top. Each frame, until four pass with nothing moved and no reserved box within
// 600 px of the screen waiting to draw (its `.viewport-sentinel`'s reach), thirty at most, the
// box and then the page scroll by however far it moved. The reader's own scroll, click or key
// (typing in a field aside) ends the hold.
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
    let stopped = false, frame = 0, quiet = 0;
    const gesture = event => { if (event.type !== "keydown" || !event.target.matches?.("input, textarea, select, [contenteditable]")) stopped = true; };
    const near = () => [...document.querySelectorAll("[data-pending=true]")].some(box => {
        const r = box.getBoundingClientRect();
        return r.bottom > r.top && r.right > r.left && r.bottom > -600 && r.top < innerHeight + 600 && r.right > 0 && r.left < innerWidth;
    });
    const hold = () => {
        if (!stopped && anchor.isConnected) {
            let moved = anchor.getBoundingClientRect().top - top;
            quiet = Math.abs(moved) >= 0.5 || near() ? 0 : quiet + 1;
            if (Math.abs(moved) >= 0.5 && scroller) {
                const before = scroller.scrollTop;
                scroller.scrollTop = before + Math.round(moved);
                moved -= scroller.scrollTop - before;
            }
            if (Math.abs(moved) >= 0.5 && page) scrollBy(0, moved);
            if (++frame < 30 && quiet < 4) return requestAnimationFrame(hold);
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

// A task page's box asks for its task's timeline (timeline_slot.rs: owner, slug, revision), and
// says when it is gone (`kind: "close"`). Boxes are numbered as they mount, so an ask from a box
// older than the one held, or from one that has closed, came late and is dropped. The timeline's
// root sits in the box of the task asked for, else hidden at the body's end, kept for the next box:
// held here, as a box the page takes down takes the root out of the document with it.
let timelineRoot, timelineClosed = 0;
function placeTimeline() {
    const root = timelineRoot ||= mountFor("timeline");
    const request = window.reviewTimelineRequest, slot = request && document.getElementById("timeline-slot");
    if (slot && slot.dataset.slug === request.slug) {
        if (root.parentElement !== slot) slot.append(root);
        root.hidden = false;
    } else {
        root.hidden = true;
        if (root.parentElement !== document.body) document.body.append(root);
    }
}
window.reviewTimeline = request => {
    const held = window.reviewTimelineRequest;
    if (request.kind === "close") {
        timelineClosed = Math.max(timelineClosed, request.owner);
        if (held?.owner !== request.owner) return;
        window.reviewTimelineRequest = null;
    } else if (request.owner <= timelineClosed || request.owner < (held?.owner ?? 0)) return;
    else window.reviewTimelineRequest = request;
    placeTimeline();
    if (!window.reviewTimelineRequest) return dispatchEvent(new Event("review:timeline"));
    const load = () => {
        const loading = ensure("timeline");
        placeTimeline();
        loading.then(() => { placeTimeline(); dispatchEvent(new Event("review:timeline")); });
    };
    if (persistent.has("timeline")) load(); else settled(load);
};

// Once the first page has drawn its data (none of its reads in flight for half a second, the last
// of them answered) and the
// browser is idle, the persistent modules' files go into the HTTP cache at the lowest priority,
// nothing evaluated, so a first open waits on no network: the chat's first, then, once its wasm
// (or its own load, if a click came first) is in, the menus', so no two share a slow link. A
// browser asking to save data, or a 2g link, skips it; the first open then fetches them. A
// browser without idle callbacks warms them at once; one without prefetch (Safari) ignores it.
let warmed = false;
function warm() {
    const link = navigator.connection, kinds = Object.keys(PERSISTENT).filter(kind => !PERSISTENT[kind].cold);
    if (warmed || link?.saveData || /2g/.test(link?.effectiveType)) return;
    warmed = true;
    (window.requestIdleCallback || setTimeout)(function next() {
        if (reads || Date.now() - answered < 500) return setTimeout(next, 500);
        const kind = kinds.shift();
        if (!kind) return;
        if (persistent.has(kind)) return persistent.get(kind).then(next);
        const { script, wasm, styles } = modules[kind];
        for (const href of [wasm, script, ...styles]) {
            if (!document.querySelector(`link[href="${href}"]`)) document.head.insertAdjacentHTML("beforeend", `<link rel="prefetch" href="${href}">`);
        }
        const last = document.querySelector(`link[rel=prefetch][href="${wasm}"]`);
        if (last) last.onload = last.onerror = next; else next();
    }, { timeout: 5000 });
}

// The routes the Symphony answers itself, symphony_api::SYMPHONY_ROUTES word for word (a test
// keeps them equal): they win over every entrypoint.
const SYMPHONY_ROUTES = ["tasks", "task", "agents", "queue", "api", "assets", "static", "thumb", "x", "artifacts/uploads", "app", "doc", "history", "diff", "wasm", "_dioxus", "favicon.ico"];

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
    if (path.startsWith("task/") || clean === "agents") return "tasks";
    if (clean === "queue") return "queue";
    const view = project();
    const pages = (view.nav || []).map(item => item.href.replace(/^\/+|\/+$/g, "")).filter(Boolean);
    if (pages.includes(clean)) return "docs";
    // A document or folder page of an entrypoint (ProjectView::is_document_route): a trailing
    // slash or an entrypoint's own route names a folder, whatever its name holds.
    const urls = view.entrypoints?.length ? view.entrypoints : ["docs"];
    const name = clean.split("/").pop();
    if (!SYMPHONY_ROUTES.concat(pages).some(taken => within(taken, clean))
        && urls.some(url => within(url, clean))
        && (path.endsWith("/") || urls.includes(clean) || !name.includes(".") || name.endsWith(".md"))) return "docs";
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
        // Keep the previous page while its replacement loads. Its styles are requested with its
        // script and wasm; insets and row geometry must apply before page effects can anchor.
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

document.addEventListener("click", event => {
    if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    const link = event.target instanceof Element && event.target.closest("a[href]");
    if (!link || link.hasAttribute("download") || link.dataset.native === "true" || (link.target && link.target !== "_self")) return;
    const url = new URL(link.href, location.href);
    if (url.origin !== location.origin || !ours(url.pathname) || !route(url.pathname)) return;
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
