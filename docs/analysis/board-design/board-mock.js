// Shared drawing for the board mock-ups: one card, one "waits on" line, the counts and the banner.
// Each page (a-stuck-lane.html, b-stuck-in-place.html, c-who-acts-next.html) lays them out its own way.
(function () {
  const B = window.BOARD;
  const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const age = (h) => (h < 48 ? `${h} h` : `${Math.round(h / 24)} d`);
  const OLD = 7 * 24; // a week or more is drawn in red
  const STAGES = ["concept", "design", "ready", "in_progress", "review", "done"];
  const NAME = { backlog: "Backlog", concept: "Concept", design: "Design", ready: "Ready", in_progress: "In progress", review: "Review", done: "Done", blocked: "Blocked", cancelled: "Cancelled" };
  const kind = (t) => (t.s === "done" ? "done" : t.w ? t.w.k : "nobody");
  // Who the line names, in the words the card uses.
  const on = (t) => {
    const w = t.w;
    if (!w) return "Done";
    return { working: "Working", you: "On you", nobody: "On nobody", queued: "Queued", parked: "Parked" }[w.k] ?? `On ${w.who}`;
  };
  const why = (t) => {
    const w = t.w;
    if (!w) return "";
    if (w.k === "working") return `${w.why} ${w.who}`;
    if (w.k === "parked") return w.why.replace(/^parked:? ?/, "");
    if (w.k === "nobody" && w.who) return `${w.who} left · ${w.why.replace(/,? ?and it left|; its orchestrator left| in a session that left/, "")}`;
    return w.why;
  };
  const open = B.tasks.filter((t) => t.s !== "done");
  const byAge = (a, b) => b.h - a.h || a.p - b.p;
  const byPrio = (a, b) => a.p - b.p || b.h - a.h;
  const muted = (t) => ["queued", "parked", "done"].includes(kind(t));
  // The line every option draws under a card's title.
  const waitLine = (t) => `<div class="wait"><span class="on">${esc(on(t))}</span><span class="why">${esc(why(t))}</span><span class="age${t.h >= OLD && !muted(t) ? " old" : ""}">${t.s === "done" ? "today" : age(t.h)}</span></div>`;
  const reviews = (t) => (t.rounds ? `<span class="tag rounds">${t.rounds} review${t.rounds > 1 ? "s" : ""}</span>` : "");
  const card = (t, o = {}) => {
    const blocked = o.blockedTag && t.s === "blocked";
    const tags = (blocked ? `<span class="tag blocked">blocked</span>` : "") + reviews(t);
    return `<a class="tcard p${t.p} k-${kind(t)}${blocked ? " is-blocked" : ""}" href="#"><div class="tt">${tags}${esc(t.title)}</div><span class="sl">${esc(t.slug)} <b class="pr">p${t.p}</b></span>${waitLine(t)}</a>`;
  };
  // Who the open work waits on, in the order the strip and the groups use: what needs someone
  // first, then work going on, then work that waits by design.
  const GROUPS = [
    { k: "you", label: "On you", chip: "on you" },
    { k: "nobody", label: "On nobody", chip: "on nobody" },
    { k: "check", label: "On the nightly", chip: "on the nightly" },
    { k: "task", label: "On other tasks", chip: "on other tasks" },
    { k: "working", label: "Working", chip: "working" },
    { k: "queued", label: "Queued", chip: "queued" },
    { k: "parked", label: "Parked", chip: "parked" },
  ];
  const count = (list, k) => list.filter((t) => kind(t) === k).length;
  const strip = (list, lead) => `<div class="stuck">${lead}${GROUPS.map((g) => `<a class="chip k-${g.k}" href="#">${g.chip} <i>${count(list, g.k)}</i></a>`).join("")}</div>`;
  // One cause said once: the board's orchestrators are gone, so ready work cannot start.
  const unstarted = open.filter((t) => t.s === "ready" && kind(t) === "queued").length;
  const banner = () => `<div class="banner"><b>No orchestrator is live.</b> ${unstarted} of the ${B.counts.ready} ready tasks wait to start · sable-finch-625 stale, glacial-heron-497 left</div>`;
  // The app's own compact header (frame.css, data-layout-header=compact), names as they are today.
  const header = () => `<nav class="hdr"><a>Change board</a><a>⌂ board</a><a>≡ docs</a><a class="on">▦ tasks</a><a>◉ agents</a><a>⬡ hulls</a><span class="end">activity</span></nav>`;
  const head = (extra = "") => `<div class="head"><h1>Tasks</h1><span class="search">Search tasks</span><span class="sel">Done: 24 h</span>${extra}</div>`;
  window.Mock = { B, esc, age, OLD, STAGES, NAME, kind, on, why, open, byAge, byPrio, card, waitLine, reviews, GROUPS, count, strip, banner, header, head };
})();
