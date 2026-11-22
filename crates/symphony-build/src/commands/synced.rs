//! `symphony-build synced BASE [PATH…]`: how long each page of a project takes, from its open to
//! synced, on a browser that has been there before. A private headless Chrome (the shot's driver,
//! crates/chrome-devtools) opens each page once, so that the page keeps what it drew on the device,
//! then opens it again `--runs` times, each a new load of the page with that device's copy, and
//! records, from the navigation's start: the first paint, the copy from the device drawn, each
//! request (queued, its first byte, its end), each read of the device's store, the feed's stream
//! (made, open, first message), each part's first answer drawn and the part that matters most
//! among them, the long tasks and the React commits, and the moment the page is synced; beside each
//! visit, the machine's load and CPU busy and the memory of the tab, the server and its board
//! databases ([`machine`]). The table it prints is the same each run, so a fix is judged by the
//! table before it and after it, measured the same way on the same machine: each run is kept as a
//! record in the project's artifacts folder and printed beside the last one kept of the same
//! address ([`kept`]). It reads the pages only: it starts, stops or changes no server.
//!
//! Synced is the page's own word for it, the same in both clients: every `data-ready` region drawn
//! (none `loading` or `failed`), none still `data-cache=device`, and every `data-feed` `live`. A
//! part is the page's own word too: a page drawn from parts (the client's `parts.ts`) marks each
//! one's first answer drawn from the server (`part KEY`) and the one that matters most
//! (`first-part`), which this prints beside drawn, checked and synced; a page of no parts has none.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use symphony::project::Project;
use symphony_args::{Flag, parse};
use symphony_chrome::{Chrome, Leash};

use machine::{Memory, Watched, busy_share, cpu_ticks, load_now, load_said};

mod kept;
mod machine;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: symphony-build synced [-h] BASE [PATH ...] [--runs N] [--timeout SECS] \
[--tabs N] [--note TEXT] [--against RUN]";
const HELP: &str = "How long each page of a project takes, from its open to synced, on a browser
that has been there before: a private headless Chrome opens each page once, so that it keeps what
it drew on this device, then opens it again --runs times and records, from the navigation's start,
the first paint, the copy from the device drawn, every request and read of the device's store, the
feed's stream, each part's first answer drawn (the page's own first-part mark for the part that
matters most), the long tasks and the React commits, and the moment the page is synced (every
data-ready region drawn, none data-cache=device, every data-feed live). It prints one row a page
(the first visit's part and synced, each return's drawn, part, checked and synced, in ms; - for a
page of no parts) and the waterfall of the run whose synced is the median, each visit with the
machine's load and CPU busy and the memory of the tab, the server and its board databases. Each run is kept as a record in the project's artifacts folder (synced/) and
printed beside the last run of the same BASE kept there, or the one --against names. It only reads
the pages: no server is started, stopped or changed.

positional arguments:
  BASE                 the project's address (http://127.0.0.1:8765/symphony)
  PATH                 the pages under BASE (default: / /tasks /queue /reports /agents
                       /docs/workflow/TASKS.md; a task's page is named, /task/SLUG)

options:
  -h, --help           show this help message and exit
  --runs N             the return visits measured a page, after its first (default 3)
  --timeout SECS       how long one visit may take to be synced before it is said unsynced
                       (default 120)
  --tabs N             keep N other tabs of the project's home open in the same browser, each
                       following its feed, while the page is measured (default 0)
  --note TEXT          a few words the kept run says of itself (\"before the outline fix\")
  --against RUN        print this run beside the kept run RUN (its file, or its name in synced/)
                       rather than the last one kept of the same BASE: a server started on port 0
                       has another BASE once it is built again, so name the run before it

Chrome is SYMPHONY_CHROME, else the first Chrome found on this machine.
Exit 0 with the table printed, 1 when the browser failed, 2 for a command line it cannot run.";

/// The pages measured when none is named: one of each kind every project has. A task's page
/// (`/task/SLUG`) is named, as no task is on every board.
const PAGES: [&str; 6] = [
    "/",
    "/tasks",
    "/queue",
    "/reports",
    "/agents",
    "/docs/workflow/TASKS.md",
];

const FLAGS: [Flag; 5] = [
    Flag {
        name: "--runs",
        value: true,
    },
    Flag {
        name: "--tabs",
        value: true,
    },
    Flag {
        name: "--timeout",
        value: true,
    },
    Flag {
        name: "--note",
        value: true,
    },
    Flag {
        name: "--against",
        value: true,
    },
];

/// What runs in each document before its own scripts: the page's own markers watched, its feed's
/// streams, its reads of the device's store, its long tasks and its React commits recorded, each at
/// `performance.now()`, in `window.__synced`.
const PROBE: &str = r#"(() => {
  if (window.__synced) return;
  const now = () => performance.now();
  const log = { drawn: null, checked: null, synced: null, unsynced: null, last: "", states: [], feeds: [], reads: [],
    fetches: [], reading: 0, bodies: 0, answered: 0, keeping: 0, commits: 0, commitsSynced: null,
    longtasks: [] };
  Object.defineProperty(window, "__synced", { value: log });
  try { performance.setResourceTimingBufferSize(10000); } catch (_) {}
  window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
    isDisabled: false, supportsFiber: true, renderers: new Map(),
    inject(renderer) { const id = this.renderers.size + 1; this.renderers.set(id, renderer); return id; },
    onCommitFiberRoot() { log.commits += 1; },
    onCommitFiberUnmount() {}, onPostCommitFiberRoot() {}, onScheduleFiberRoot() {}, setStrictMode() {},
    checkDCE() {},
  };
  try {
    new PerformanceObserver((list) => {
      for (const task of list.getEntries()) log.longtasks.push([task.startTime, task.duration]);
    }).observe({ type: "longtask", buffered: true });
  } catch (_) {}
  const follow = (Native, kind) => class extends Native {
    constructor(...args) {
      super(...args);
      const feed = { kind, url: String(args[0]), made: now(), open: null, first: null, messages: 0, ended: null };
      log.feeds.push(feed);
      this.addEventListener("open", () => { feed.open ??= now(); schedule(); });
      this.addEventListener("message", () => { feed.messages += 1; feed.first ??= now(); });
      const ended = () => { feed.ended ??= now(); schedule(); };
      this.addEventListener("error", ended);
      this.addEventListener("close", ended);
      const close = this.close.bind(this);
      this.close = (...args) => { ended(); return close(...args); };
    }
  };
  if (window.WebSocket) window.WebSocket = follow(window.WebSocket, "socket");
  if (window.EventSource) window.EventSource = follow(window.EventSource, "events");
  let store;
  Object.defineProperty(window, "symphonyCache", {
    configurable: true,
    get: () => store,
    set(given) {
      if (!given || typeof given.read !== "function" || typeof given.keep !== "function") {
        store = given;
        return;
      }
      const { read, keep } = given;
      store = { ...given,
        read: async (...args) => {
          const entry = { part: `${args[2]} ${args[3]}`, start: now(), end: null, hit: false };
          log.reads.push(entry);
          try {
            const kept = await read(...args);
            entry.hit = kept != null;
            return kept;
          } finally { entry.end = now(); }
        },
        keep: async (...args) => {
          log.keeping += 1;
          try { return await keep(...args); } finally { log.keeping -= 1; }
        },
      };
    },
  });
  const nativeFetch = window.fetch;
  window.fetch = function (input, init) {
    const url = String(typeof input === "string" || input instanceof URL ? input : input.url);
    // A read ahead (the shell's, at the lowest priority) is no read of the page's own.
    const own = /\/api\//.test(url) && !(init && init.priority === "low");
    const entry = { url, start: now(), end: null, status: null, own };
    log.fetches.push(entry);
    if (own) log.reading += 1;
    const answer = nativeFetch.apply(this, arguments);
    answer.then((response) => { entry.status = response.status; }, () => { entry.status = "failed"; })
      .finally(() => {
        entry.end = now();
        if (own) {
          log.reading -= 1;
          log.answered += 1;
        }
        schedule();
      });
    return answer;
  };
  for (const name of ["json", "text", "arrayBuffer", "blob"]) {
    const native = Response.prototype[name];
    Response.prototype[name] = function () {
      log.bodies += 1;
      const body = native.call(this);
      // Settled either way, and a body that fails is the page's to hear, not this count's.
      const settled = () => { log.bodies -= 1; schedule(); };
      body.then(settled, settled);
      return body;
    };
  }
  // The pages that follow no feed, by their regions' words.
  const unfed = new Set(["reports", "runs", "run", "settings", "html", "folder"]);
  const look = () => {
    const ready = [...document.querySelectorAll("[data-ready]")].map((node) => node.getAttribute("data-ready"));
    const device = document.querySelector('[data-cache="device"]') !== null;
    const stale = document.querySelector('[data-feed]:not([data-feed="live"])') !== null;
    const open = log.feeds.some((feed) => feed.open !== null && feed.ended === null);
    const reading = log.reading + log.bodies;
    const state = `${ready.join(",")}${device ? " device" : ""}${stale ? " stale" : ""}` +
      `${open ? " open" : ""}${reading > 0 ? ` reading ${reading}` : ""}`;
    if (state === log.last) return;
    log.last = state;
    const at = now();
    log.states.push([at, state]);
    if (device && log.drawn === null) log.drawn = at;
    const drawn = ready.length > 0 && !ready.some((word) => word === "loading" || word === "failed");
    const fed = open || ready.every((word) => unfed.has(word));
    // Checked is the client's own word (no copy from the device left unanswered, the feed open);
    // synced is that with every read the page made answered too.
    const checked = drawn && !device && !stale && fed && log.answered > 0;
    if (checked && log.checked === null) log.checked = at;
    const synced = checked && reading === 0;
    if (synced && log.synced === null) {
      log.synced = at;
      log.commitsSynced = log.commits;
    } else if (!synced && log.synced !== null && log.unsynced === null) log.unsynced = at;
  };
  // After the task that changed something: a read's answer sets off the next read in its own turn.
  let looking = false;
  function schedule() {
    if (looking) return;
    looking = true;
    setTimeout(() => { looking = false; look(); }, 0);
  }
  new MutationObserver(schedule).observe(document, { subtree: true, childList: true, attributes: true,
    attributeFilter: ["data-ready", "data-cache", "data-feed"] });
})();"#;

/// What the page recorded, with its own marks of its parts and its paint and resource timings, as
/// one value.
const COLLECT: &str = r#"(() => {
  const log = window.__synced;
  const navigation = performance.getEntriesByType("navigation")[0];
  const marks = performance.getEntriesByType("mark");
  return { drawn: log.drawn, checked: log.checked, synced: log.synced, unsynced: log.unsynced, last: log.last,
    states: log.states, feeds: log.feeds, reads: log.reads, fetches: log.fetches, commits: log.commits,
    commitsSynced: log.commitsSynced, longtasks: log.longtasks,
    firstPart: marks.find((mark) => mark.name === "first-part")?.startTime ?? null,
    parts: marks.filter((mark) => mark.name.startsWith("part ")).map((mark) => [mark.name.slice(5), mark.startTime]),
    build: document.querySelector("script[src*='symphony-shell-']")?.getAttribute("src") ?? null,
    paint: Object.fromEntries(performance.getEntriesByType("paint").map((entry) => [entry.name, entry.startTime])),
    document: navigation ? { sent: navigation.requestStart, first: navigation.responseStart,
      end: navigation.responseEnd, loaded: navigation.domContentLoadedEventEnd,
      bytes: navigation.transferSize } : null,
    requests: performance.getEntriesByType("resource").map((entry) => ({ url: entry.name,
      kind: entry.initiatorType, start: entry.startTime, sent: entry.requestStart,
      first: entry.responseStart, end: entry.responseEnd, bytes: entry.transferSize,
      status: entry.responseStatus ?? 0 })) };
})()"#;

/// The renderer's own counters (`Performance.getMetrics`) a visit reads before and after: its main
/// thread's tasks and scripts on the wall, and its main thread's and its process's CPU.
const METRICS: [&str; 4] = [
    "TaskDuration",
    "ScriptDuration",
    "ThreadTime",
    "ProcessTime",
];

/// The renderer's counter of the tab's JavaScript heap in use, read after a visit.
const HEAP: &str = "JSHeapUsedSize";

/// The command line, read.
#[derive(Debug, PartialEq)]
struct Asked {
    base: String,
    pages: Vec<String>,
    runs: usize,
    tabs: usize,
    timeout: Duration,
    note: Option<String>,
    against: Option<String>,
}

/// One visit to a page: which (0 the first, which keeps the device's copy), the load it began
/// under, what the page recorded, the renderer's counters it spent, the machine's CPU busy while
/// it ran and the memory it left, and why it ended unsynced.
#[derive(Clone, Debug, Default)]
struct Visit {
    run: usize,
    load: Option<[f64; 3]>,
    probe: Value,
    spent: Vec<(String, f64)>,
    busy: Option<f64>,
    memory: Memory,
    error: Option<String>,
}

impl Visit {
    /// When the page drew the device's copy, when it said it checked it (no copy from the device
    /// unanswered, its feed open) and when it was synced (every read it made answered too), in ms
    /// from its navigation.
    fn drawn(&self) -> Option<f64> {
        self.probe["drawn"].as_f64()
    }

    fn checked(&self) -> Option<f64> {
        self.probe["checked"].as_f64()
    }

    fn synced(&self) -> Option<f64> {
        self.probe["synced"].as_f64()
    }

    /// When the page drew the server's first answer for the part that matters most on it (its
    /// `first-part` mark), in ms from its navigation; none on a page of no parts.
    fn first_part(&self) -> Option<f64> {
        self.probe["firstPart"].as_f64()
    }

    /// When the page, once synced, was no longer: a region drawn again from the device or as
    /// loading, the feed lost, or a read begun.
    fn unsynced(&self) -> Option<f64> {
        self.probe["unsynced"].as_f64()
    }

    /// The page's own reads that failed, each as its address and its status: a status other than
    /// 2xx or 304, or no answer at all. A read still out when the page was read is none of them.
    fn failed(&self) -> Vec<String> {
        self.probe["fetches"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|fetch| fetch["own"] == true)
            .filter_map(|fetch| {
                let status = &fetch["status"];
                let failed = match status.as_u64() {
                    Some(code) => !(200..300).contains(&code) && code != 304,
                    None => status.is_string(),
                };
                failed.then(|| {
                    let said = status
                        .as_u64()
                        .map_or_else(|| "no answer".to_string(), |code| code.to_string());
                    format!(
                        "{} {said}",
                        short(fetch["url"].as_str().unwrap_or_default())
                    )
                })
            })
            .collect()
    }
}

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let asked = match read(args) {
        Ok(Some(asked)) => asked,
        Ok(None) => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        Err(problem) => return refuse(err, &problem),
    };
    let Some(binary) = symphony_chrome::find().filter(|binary| Path::new(binary).is_file()) else {
        let _ = writeln!(
            err,
            "symphony-build synced: no Chrome on this machine: name its executable with \
             SYMPHONY_CHROME"
        );
        return 1;
    };
    // Where the runs are kept, and the one this run is printed beside when it is named, read
    // before anything is measured.
    let root = std::env::current_dir()
        .ok()
        .and_then(|here| Project::find(&here));
    let folder = root.as_deref().and_then(|root| {
        Project::load(root)
            .ok()
            .map(|project| root.join(project.artifact(kept::FOLDER)))
    });
    let against = match &asked.against {
        Some(name) => {
            let within = folder.clone().unwrap_or_default();
            match kept::named(&within, name) {
                Ok(found) => Some(found),
                Err(problem) => return refuse(err, &problem),
            }
        }
        None => None,
    };
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
    let started = (load_now(), cpu_ticks());
    let mut measured = Vec::new();
    let mut failed = false;
    for page in &asked.pages {
        let url = format!("{}{page}", asked.base);
        match measure(&binary, &url, &asked) {
            Ok(visits) => {
                let _ = write!(out, "{}", waterfall(page, &visits));
                let _ = out.flush();
                measured.push((page.clone(), visits));
            }
            Err(problem) => {
                let _ = writeln!(err, "symphony-build synced: {url}: {problem}");
                failed = true;
            }
        }
    }
    let ended = (load_now(), cpu_ticks());
    let _ = write!(out, "{}", table(&asked.base, &measured, ended.0));
    let run = kept::Run {
        base: &asked.base,
        runs: asked.runs,
        tabs: asked.tabs,
        note: asked.note.as_deref(),
        at: stamp(epoch),
        epoch,
        checkout: root.as_deref().and_then(checkout),
        load: [started.0, ended.0],
        busy: busy_share(started.1, ended.1),
    };
    let record = kept::record(&run, &measured);
    match (&root, &folder) {
        (Some(root), Some(folder)) => {
            let earlier = against.or_else(|| kept::earlier(folder, &asked.base));
            let path = folder.join(kept::file_name(&run.at));
            let text = serde_json::to_string_pretty(&record).unwrap_or_default();
            if let Err(error) =
                std::fs::create_dir_all(folder).and_then(|()| std::fs::write(&path, text))
            {
                let _ = writeln!(
                    err,
                    "symphony-build synced: keeping the run in {}: {error}",
                    path.display()
                );
                return 1;
            }
            let shown = |path: &Path| {
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            };
            let _ = writeln!(out, "\nkept as {}", shown(&path));
            match earlier {
                Some((path, then)) => {
                    let _ = write!(out, "{}", kept::compare(&shown(&path), &then, &record));
                }
                None => {
                    let _ = writeln!(
                        out,
                        "no earlier run of {} kept to print it beside",
                        asked.base
                    );
                }
            }
        }
        _ => {
            let _ = writeln!(
                err,
                "symphony-build synced: no project at or above this folder, so the run is not kept"
            );
            if let Some((path, then)) = against {
                let _ = write!(
                    out,
                    "{}",
                    kept::compare(&path.display().to_string(), &then, &record)
                );
            }
        }
    }
    i32::from(failed)
}

/// A command line it cannot run, refused with its usage: exit 2.
fn refuse(err: &mut dyn Write, problem: &str) -> i32 {
    let _ = writeln!(err, "{USAGE}\nsymphony-build synced: error: {problem}");
    2
}

/// `epoch` as the local `YYYY-MM-DD HH:MM:SS` a kept run is named by.
fn stamp(epoch: i64) -> String {
    let minute = symphony::clock::stamp_at(libc::time_t::from(epoch));
    format!("{minute}:{:02}", epoch.rem_euclid(60))
}

/// The commit the checkout at `root` stands at.
fn checkout(root: &Path) -> Option<String> {
    let done = symphony::children::output(
        symphony::children::git()
            .args(["rev-parse", "HEAD"])
            .current_dir(root),
    )
    .ok()?;
    let commit = String::from_utf8_lossy(&done.stdout).trim().to_string();
    (done.status.success() && !commit.is_empty()).then_some(commit)
}

/// The command line as what to measure; `None` when it asks for help.
fn read(args: &[String]) -> Result<Option<Asked>, String> {
    let parsed = parse(args, &FLAGS)?;
    if parsed.help {
        return Ok(None);
    }
    let Some((base, pages)) = parsed.positionals.split_first() else {
        return Err("the following arguments are required: BASE".into());
    };
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err(format!(
            "argument BASE: {base:?} is not an http address (http://127.0.0.1:8765/symphony)"
        ));
    }
    let number = |name: &str, default: u64, least: u64| -> Result<u64, String> {
        parsed.value(name).map_or(Ok(default), |given| {
            given
                .parse::<u64>()
                .ok()
                .filter(|value| *value >= least)
                .ok_or_else(|| {
                    format!("argument {name}: {given:?} is not a whole number of {least} or more")
                })
        })
    };
    let pages = if pages.is_empty() {
        PAGES.iter().map(|page| (*page).to_string()).collect()
    } else {
        pages.to_vec()
    };
    if let Some(page) = pages.iter().find(|page| !page.starts_with('/')) {
        return Err(format!("argument PATH: {page:?} does not start with /"));
    }
    Ok(Some(Asked {
        base: base.trim_end_matches('/').to_string(),
        pages,
        runs: usize::try_from(number("--runs", 3, 1)?).unwrap_or(usize::MAX),
        tabs: usize::try_from(number("--tabs", 0, 0)?).unwrap_or(usize::MAX),
        timeout: Duration::from_secs(number("--timeout", 120, 1)?),
        note: parsed.value("--note").map(str::to_string),
        against: parsed.value("--against").map(str::to_string),
    }))
}

/// Every visit to `url` in a browser of its own: the first, then the returns.
fn measure(binary: &str, url: &str, asked: &Asked) -> Result<Vec<Visit>, String> {
    let program = std::env::current_exe()
        .map_err(|error| format!("this program's own path, to leash Chrome: {error}"))?;
    let mut chrome = Chrome::launch(binary, &Leash::new(program, &[]).quiet())
        .map_err(|error| error.to_string())?;
    chrome
        .call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": PROBE }),
        )
        .map_err(|error| error.to_string())?;
    chrome
        .call("Performance.enable", json!({}))
        .map_err(|error| error.to_string())?;
    // The other tabs share the browser's connections to the server with the page measured.
    let home = format!("{}/", asked.base);
    for _ in 0..asked.tabs {
        chrome
            .browser_call("Target.createTarget", json!({ "url": home }))
            .map_err(|error| format!("another tab: {error}"))?;
    }
    if asked.tabs > 0 {
        chrome
            .call("Page.bringToFront", json!({}))
            .map_err(|error| error.to_string())?;
    }
    let mut visits: Vec<Visit> = Vec::new();
    // Found once the first visit has had the server open its connections to its databases.
    let mut watched: Option<Watched> = None;
    for run in 0..=asked.runs {
        let mut visit = visit(&mut chrome, url, run, asked.timeout)?;
        let found = watched.get_or_insert_with(|| Watched::find(&asked.base));
        visit.memory = found.memory(visit.memory.heap);
        let answered = !visit.probe.is_null();
        visits.push(visit);
        // A page that answered nothing would answer the next visit no better.
        if !answered {
            break;
        }
    }
    Ok(visits)
}

/// One visit: the page opened, waited for until synced (or `timeout`), and, once the copy it keeps
/// on the device is written, what it recorded read.
fn visit(chrome: &mut Chrome, url: &str, run: usize, timeout: Duration) -> Result<Visit, String> {
    let load = load_now();
    let ticks = cpu_ticks();
    let before = metrics(chrome)?;
    chrome.set_deadline(Some(Instant::now() + timeout));
    let mut error = match chrome.navigate(url, timeout) {
        Ok(()) => None,
        Err(error) => Some(format!("not loaded: {error}")),
    };
    if error.is_none() {
        let synced =
            "window.__synced && window.__synced.synced !== null && window.__synced.keeping === 0";
        error = match chrome.wait_for(synced, timeout) {
            Ok(true) => None,
            Ok(false) => Some(format!("not synced within {} s", timeout.as_secs())),
            Err(error) if error.past_deadline() => {
                Some(format!("not synced within {} s", timeout.as_secs()))
            }
            Err(error) => Some(error.to_string()),
        };
    }
    // What the page recorded is read within the visit's allowance again: a page that answers
    // nothing by then is said so, with nothing recorded.
    chrome.set_deadline(Some(Instant::now() + timeout));
    let probe = chrome.eval(COLLECT).unwrap_or_else(|failed| {
        error.get_or_insert_with(|| format!("nothing read of the page: {failed}"));
        Value::Null
    });
    // A page whose renderer answers nothing more leaves its counters unread, the visit kept.
    let after = if probe.is_null() {
        HashMap::new()
    } else {
        metrics(chrome).unwrap_or_else(|failed| {
            error.get_or_insert_with(|| format!("its counters unread: {failed}"));
            HashMap::new()
        })
    };
    chrome.set_deadline(None);
    let busy = busy_share(ticks, cpu_ticks());
    let spent = METRICS
        .iter()
        .filter_map(|name| {
            let spent = after.get(*name)? - before.get(*name)?;
            Some(((*name).to_string(), spent * 1000.0))
        })
        .collect();
    Ok(Visit {
        run,
        load,
        probe,
        spent,
        busy,
        memory: Memory {
            heap: after.get(HEAP).copied(),
            ..Memory::default()
        },
        error,
    })
}

/// The renderer's counters (`Performance.getMetrics`) by name, its times in seconds.
fn metrics(chrome: &mut Chrome) -> Result<HashMap<String, f64>, String> {
    let said = chrome
        .call("Performance.getMetrics", json!({}))
        .map_err(|error| error.to_string())?;
    Ok(said["metrics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|metric| {
            Some((
                metric["name"].as_str()?.to_string(),
                metric["value"].as_f64()?,
            ))
        })
        .collect())
}

/// `value` in ms as the table writes it: whole, or `-` for none.
fn ms(value: Option<f64>) -> String {
    value.map_or_else(|| "-".into(), |value| format!("{value:.0}"))
}

/// The return visits' median by synced, an unsynced visit counting as the slowest.
fn median(visits: &[Visit]) -> Option<&Visit> {
    let mut returns: Vec<&Visit> = visits.iter().filter(|visit| visit.run > 0).collect();
    returns.sort_by(|a, b| {
        let key = |visit: &Visit| visit.synced().unwrap_or(f64::INFINITY);
        key(a).total_cmp(&key(b))
    });
    returns.get(returns.len().saturating_sub(1) / 2).copied()
}

/// A request's address as the table names it: its path and query, the origin left out, a long
/// one cut short.
fn short(url: &str) -> String {
    let path = url
        .split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|at| &rest[at..]))
        .unwrap_or(url);
    if path.chars().count() > 72 {
        format!("{}…", path.chars().take(71).collect::<String>())
    } else {
        path.to_string()
    }
}

/// One row of a visit's waterfall: when it starts, then what it is.
#[derive(Debug, PartialEq)]
struct Row {
    at: f64,
    said: String,
}

/// What a visit did up to its synced, in the order it began: the document, each request started
/// before synced (static files the browser's cache answered left out), each read of the device's
/// store, each feed stream, each long task, each part's first answer drawn and the part that
/// matters most, the copy drawn and the moment synced.
fn rows(visit: &Visit) -> Vec<Row> {
    let probe = &visit.probe;
    let until = visit.synced().unwrap_or(f64::INFINITY);
    let mut rows = Vec::new();
    let number = |value: &Value| value.as_f64();
    let document = &probe["document"];
    if let (Some(sent), Some(first), Some(end)) = (
        number(&document["sent"]),
        number(&document["first"]),
        number(&document["end"]),
    ) {
        rows.push(Row {
            at: 0.0,
            said: format!(
                "the page          sent {sent:.0}  first byte {first:.0}  end {end:.0}  \
                 parsed {}",
                ms(number(&document["loaded"]))
            ),
        });
    }
    for request in probe["requests"].as_array().into_iter().flatten() {
        let Some(start) = number(&request["start"]) else {
            continue;
        };
        let bytes = number(&request["bytes"]).unwrap_or(0.0);
        let url = request["url"].as_str().unwrap_or_default();
        // A file the browser's cache answered costs no wait on the server.
        if start > until || (bytes == 0.0 && url.contains("/assets/")) {
            continue;
        }
        let sent = number(&request["sent"]).unwrap_or(start);
        let first = number(&request["first"]).unwrap_or(sent);
        let end = number(&request["end"]).unwrap_or(first);
        let status = request["status"].as_u64().unwrap_or(0);
        rows.push(Row {
            at: start,
            said: format!(
                "{}  queued {:.0}  server {:.0}  body {:.0}  end {end:.0}  {status}  {:.1} KB",
                short(url),
                (sent - start).max(0.0),
                (first - sent).max(0.0),
                (end - first).max(0.0),
                bytes / 1000.0
            ),
        });
    }
    for read in probe["reads"].as_array().into_iter().flatten() {
        let Some(start) = number(&read["start"]) else {
            continue;
        };
        if start > until {
            continue;
        }
        rows.push(Row {
            at: start,
            said: format!(
                "device read {}  end {}  {}",
                read["part"].as_str().unwrap_or_default(),
                ms(number(&read["end"])),
                if read["hit"] == true { "kept" } else { "none" }
            ),
        });
    }
    for feed in probe["feeds"].as_array().into_iter().flatten() {
        let Some(made) = number(&feed["made"]) else {
            continue;
        };
        if made > until {
            continue;
        }
        rows.push(Row {
            at: made,
            said: format!(
                "feed {} {}  open {}  first message {}",
                feed["kind"].as_str().unwrap_or_default(),
                short(feed["url"].as_str().unwrap_or_default()),
                ms(number(&feed["open"])),
                ms(number(&feed["first"]))
            ),
        });
    }
    for task in probe["longtasks"].as_array().into_iter().flatten() {
        if let (Some(start), Some(length)) = (number(&task[0]), number(&task[1]))
            && start <= until
        {
            rows.push(Row {
                at: start,
                said: format!("long task {length:.0} ms"),
            });
        }
    }
    if let Some(first) = number(&probe["paint"]["first-contentful-paint"]) {
        rows.push(Row {
            at: first,
            said: "first contentful paint".into(),
        });
    }
    for part in probe["parts"].as_array().into_iter().flatten() {
        if let (Some(key), Some(at)) = (part[0].as_str(), number(&part[1]))
            && at <= until
        {
            rows.push(Row {
                at,
                said: format!("part {key} drawn from the server"),
            });
        }
    }
    if let Some(first) = visit.first_part().filter(|at| *at <= until) {
        rows.push(Row {
            at: first,
            said: "FIRST PART: the part that matters most, drawn from the server".into(),
        });
    }
    if let Some(drawn) = visit.drawn() {
        rows.push(Row {
            at: drawn,
            said: "DRAWN from the device".into(),
        });
    }
    if let Some(checked) = visit.checked() {
        rows.push(Row {
            at: checked,
            said: "CHECKED: nothing from the device unanswered, the feed open".into(),
        });
    }
    if let Some(synced) = visit.synced() {
        rows.push(Row {
            at: synced,
            said: "SYNCED: every read answered too".into(),
        });
    }
    rows.sort_by(|a, b| a.at.total_cmp(&b.at));
    rows
}

/// The page's measurement as it is printed once measured: each visit's drawn and synced, and the
/// median return's waterfall.
fn waterfall(page: &str, visits: &[Visit]) -> String {
    let mut said = String::new();
    let _ = writeln!(said, "\n{page}");
    for visit in visits {
        let which = if visit.run == 0 {
            "first".to_string()
        } else {
            format!("return {}", visit.run)
        };
        let mut notes = String::new();
        if let Some(left) = visit.unsynced() {
            let _ = write!(notes, "  LEFT SYNCED at {left:.0}");
        }
        let failed = visit.failed();
        if !failed.is_empty() {
            let _ = write!(notes, "  READS FAILED: {}", failed.join(", "));
        }
        if let Some(error) = &visit.error {
            let _ = write!(notes, "  ({error}; last state {})", visit.probe["last"]);
        }
        let _ = writeln!(
            said,
            "  {which:<9} drawn {:>6}  part {:>6}  checked {:>6}  synced {:>6}  {}  {}  CPU busy {}  \
             {}{notes}",
            ms(visit.drawn()),
            ms(visit.first_part()),
            ms(visit.checked()),
            ms(visit.synced()),
            spent_said(visit),
            load_said(visit.load),
            visit
                .busy
                .map_or_else(|| "-".into(), |busy| format!("{busy:.0}%")),
            visit.memory.said(),
        );
    }
    if let Some(median) = median(visits) {
        let _ = writeln!(said, "  return {}, the median:", median.run);
        for row in rows(median) {
            let _ = writeln!(said, "  {:>7.0}  {}", row.at, row.said);
        }
    }
    said
}

/// What the page's main thread and process spent in a visit, and its React commits until synced.
fn spent_said(visit: &Visit) -> String {
    let spent = |name: &str| {
        visit
            .spent
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, value)| *value)
    };
    let long: f64 = visit.probe["longtasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|task| {
            task[0]
                .as_f64()
                .zip(visit.synced())
                .is_some_and(|(start, synced)| start <= synced)
        })
        .filter_map(|task| task[1].as_f64())
        .fold(0.0, |sum, length| sum + length);
    format!(
        "tasks {} ms  cpu {} ms  long {long:.0} ms  commits {}",
        ms(spent("TaskDuration")),
        ms(spent("ThreadTime")),
        visit.probe["commitsSynced"]
            .as_u64()
            .map_or_else(|| "-".into(), |commits| commits.to_string())
    )
}

/// The summary: one row a page, its first visit's part and synced and each return's drawn, part,
/// checked and synced.
fn table(base: &str, measured: &[(String, Vec<Visit>)], load: Option<[f64; 3]>) -> String {
    let mut said = String::new();
    let memory = measured
        .iter()
        .rev()
        .find_map(|(_, visits)| visits.last())
        .map(|visit| visit.memory.machine_said())
        .unwrap_or_default();
    let _ = writeln!(
        said,
        "\nopen to synced on a returning browser, {base}, ms from the navigation's start \
         ({}, {memory} at the end)",
        load_said(load)
    );
    let _ = writeln!(
        said,
        "{:<40} {:>19}  returns (drawn/part/checked/synced)",
        "page", "first (part/synced)"
    );
    // A visit whose own read failed is marked: it may read as synced, and soon, on nothing.
    let marked = |visit: &Visit| if visit.failed().is_empty() { "" } else { "!" };
    let mut any_failed = false;
    for (page, visits) in measured {
        any_failed |= visits.iter().any(|visit| !visit.failed().is_empty());
        let first = visits.iter().find(|visit| visit.run == 0);
        let returns: Vec<String> = visits
            .iter()
            .filter(|visit| visit.run > 0)
            .map(|visit| {
                format!(
                    "{}/{}/{}/{}{}",
                    ms(visit.drawn()),
                    ms(visit.first_part()),
                    ms(visit.checked()),
                    ms(visit.synced()),
                    marked(visit)
                )
            })
            .collect();
        let _ = writeln!(
            said,
            "{:<40} {:>19}{:<1} {}",
            short(page),
            format!(
                "{}/{}",
                ms(first.and_then(Visit::first_part)),
                ms(first.and_then(Visit::synced))
            ),
            first.map_or("", marked),
            returns.join("  ")
        );
    }
    if any_failed {
        let _ = writeln!(
            said,
            "! a read of the page's own failed: its visit's line above names it"
        );
    }
    said
}
