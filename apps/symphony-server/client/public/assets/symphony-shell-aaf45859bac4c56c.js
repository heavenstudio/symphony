// One history adapter; switching page kinds awaits the old runtime's cleanup.
const modules = {"board":{"script":"/assets/board-c1fc0394b4054560.js","wasm":"/assets/symphony-ui_bg-dxhcb902f3b438d78c.wasm","styles":["/assets/board-dxhefeade78e2edd3d.css","/assets/dx-components-theme-dxhc90fd9c3597d61.css","/assets/frame-dxh94475b58d9c4f0ef.css","/assets/style-dxh2b8e6760884eda3b.css","/assets/style-dxh588a9d9eb7bea867.css","/assets/style-dxh5b20fd2b62b28d.css","/assets/trigger-dxhfa692db578bda8e.css"]},"tasks":{"script":"/assets/tasks-47c7e509cec579ec.js","wasm":"/assets/symphony-ui_bg-dxhe64f85d88c4a3044.wasm","styles":["/assets/board-dxhefeade78e2edd3d.css","/assets/dx-components-theme-dxhc90fd9c3597d61.css","/assets/frame-dxh94475b58d9c4f0ef.css","/assets/media-dxhba1089dea7a236d1.css","/assets/style-dxh2b8e6760884eda3b.css","/assets/style-dxh588a9d9eb7bea867.css","/assets/style-dxh5b20fd2b62b28d.css","/assets/trigger-dxhfa692db578bda8e.css"]},"docs":{"script":"/assets/docs-11bb57e66fa71149.js","wasm":"/assets/symphony-ui_bg-dxhaa2d83d3361a2c1f.wasm","styles":["/assets/board-dxhefeade78e2edd3d.css","/assets/docs-dxh2341df6739035c1.css","/assets/dx-components-theme-dxhc90fd9c3597d61.css","/assets/frame-dxh94475b58d9c4f0ef.css","/assets/media-dxhba1089dea7a236d1.css","/assets/style-dxh2b8e6760884eda3b.css","/assets/style-dxh588a9d9eb7bea867.css","/assets/style-dxh5b20fd2b62b28d.css","/assets/trigger-dxhfa692db578bda8e.css"]},"review":{"script":"/assets/review-b2aa2a78bcc05b18.js","wasm":"/assets/symphony-ui_bg-dxhd6cda453cb4392a.wasm","styles":["/assets/board-dxhefeade78e2edd3d.css","/assets/dx-components-theme-dxhc90fd9c3597d61.css","/assets/frame-dxh94475b58d9c4f0ef.css","/assets/media-dxhba1089dea7a236d1.css","/assets/style-dxh2b8e6760884eda3b.css","/assets/style-dxh588a9d9eb7bea867.css","/assets/style-dxh5b20fd2b62b28d.css","/assets/trigger-dxhfa692db578bda8e.css"]},"queue":{"script":"/assets/queue-38cddf74c758fe61.js","wasm":"/assets/symphony-ui_bg-dxh8833f421cef363a.wasm","styles":["/assets/board-dxhefeade78e2edd3d.css","/assets/dx-components-theme-dxhc90fd9c3597d61.css","/assets/frame-dxh94475b58d9c4f0ef.css","/assets/media-dxhba1089dea7a236d1.css","/assets/queue-dxhf7936e66abf3f45d.css","/assets/style-dxh2b8e6760884eda3b.css","/assets/style-dxh588a9d9eb7bea867.css","/assets/style-dxh5b20fd2b62b28d.css","/assets/trigger-dxhfa692db578bda8e.css"]},"panel":{"script":"/assets/panel-8aba67dcde8c7759.js","wasm":"/assets/symphony-ui_bg-dxhb3c19e792b57416d.wasm","styles":["/assets/board-dxhefeade78e2edd3d.css","/assets/dx-components-theme-dxhc90fd9c3597d61.css","/assets/frame-dxh94475b58d9c4f0ef.css","/assets/media-dxhba1089dea7a236d1.css","/assets/style-dxh2b8e6760884eda3b.css","/assets/style-dxh3d38b3bceb38f78e.css","/assets/style-dxh57586b8f2aaad29.css","/assets/style-dxh588a9d9eb7bea867.css","/assets/style-dxh5b20fd2b62b28d.css","/assets/style-dxh8a0b7e49bec6bd.css","/assets/style-dxh9467f72d689fd4cd.css","/assets/style-dxhc22dba4bb7e8e964.css","/assets/style-dxhc27ce24f8128f9a.css","/assets/style-dxhcf59f468dd75bafa.css","/assets/trigger-dxhfa692db578bda8e.css"]}};
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
let panel;
const styleLoads = new Map();
const failed = new Set();

function notices() {
    return document.getElementById("shell-notices");
}

async function initialize(kind) {
    const spec = modules[kind];
    const module = await import(spec.script);
    try {
        if (failed.has(kind)) await module.default({ module_or_path: spec.wasm });
        else await window[`reviewModuleReady_${kind}`];
        failed.delete(kind);
        return window[`reviewModule_${kind}`];
    } catch (error) { failed.add(kind); throw error; }
}

function mountFor(kind) {
    const id = kind === "panel" ? "quick-root" : `review-root-${kind}`;
    let mount = document.getElementById(id);
    if (!mount) {
        mount = document.createElement("div");
        mount.id = id;
        mount.hidden = kind !== "panel";
        (kind === "panel" ? document.body : document.getElementById("main")).append(mount);
    }
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

async function ensurePanel() {
    if (panel) return panel;
    panel = (async () => {
        const spec = modules.panel;
        mountFor("panel");
        const imported = await initialize("panel");
        await styles(spec);
        await imported.run();
        document.getElementById("panel-error")?.remove();
        window.reviewPanelReady = true;
        dispatchEvent(new Event("review:panel-ready"));
        return { imported };
    })().catch(error => {
        panel = null;
        console.error("Conversation could not be loaded", error);
        if (document.getElementById("panel-error")) return;
        const notice = document.createElement("div");
        notice.id = "panel-error";
        notice.className = "shell-notice";
        notice.setAttribute("role", "alert");
        notice.textContent = "Conversation could not be loaded. ";
        const retry = document.createElement("button");
        retry.textContent = "Retry loading conversation";
        retry.onclick = () => { notice.remove(); ensurePanel(); };
        notice.append(retry);
        notices().append(notice);
    });
    return panel;
}

// Data-only overlays share the already deferred persistent panel and its owned runtime.
window.reviewOverlay = request => {
    if (request.kind === "close") {
        if (window.reviewOverlayRequest?.owner !== request.owner) return;
        window.reviewOverlayRequest = null;
        dispatchEvent(new Event("review:overlay"));
        return;
    }
    window.reviewOverlayRequest = request;
    ensurePanel().then(() => dispatchEvent(new Event("review:overlay")));
};

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

// The page's path under the project's base (ProjectView::here): the one server serves each
// project at `/<id>/`, a server of its own at `/`.
function here(path) {
    const base = project().base || "";
    return !base || path === base || path.startsWith(`${base}/`) ? path : base + path;
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
        // Keep the previous page while its replacement loads.
        const mount = mountFor(kind);
        const imported = await initialize(kind);
        // Insets and row geometry must apply before page effects can anchor.
        await styles(spec);
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
    switching = switching.catch(() => {}).then(() => show(generation, scroll)).catch(error => {
        if (generation === requested) document.documentElement.dataset.navigation = "failed";
        console.error("Page could not be loaded", error);
        let notice = document.getElementById("module-error");
        if (!notice) {
            notice = document.createElement("div");
            notice.id = "module-error";
            notice.className = "shell-notice";
            notice.setAttribute("role", "alert");
            notices().append(notice);
        }
        notice.replaceChildren(document.createTextNode("This page could not be loaded. "));
        const retry = document.createElement("button");
        retry.textContent = "Retry";
        retry.onclick = () => { notice.remove(); navigate(); };
        notice.append(retry);
    });
}

document.addEventListener("click", event => {
    if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    const link = event.target instanceof Element && event.target.closest("a[href]");
    if (!link || link.hasAttribute("download") || link.dataset.native === "true" || (link.target && link.target !== "_self")) return;
    const url = new URL(link.href, location.href);
    if (url.origin !== location.origin) return;
    url.pathname = here(url.pathname);
    if (!route(url.pathname)) return;
    event.preventDefault();
    if (document.querySelector("[data-posting=true]")) return;
    history.pushState(null, "", url);
    navigate();
});
addEventListener("popstate", navigate);
addEventListener("review:quick", () => { if (window.reviewQuickDraft?.open) ensurePanel(); });
addEventListener("review:activity", () => { if (window.reviewActivityOpen) ensurePanel(); });
addEventListener("review:activity-arrived", ensurePanel);
async function pendingStep(step) {
    await ensurePanel();
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

// Notice styles: 14b51b133c208438
