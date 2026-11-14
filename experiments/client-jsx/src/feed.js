// The live feed's contract (apps/symphony-ui/AGENTS.md, "The feed"): read `/api/board` for the
// version, read what the page needs at it, then follow `/api/feed?since=VERSION…`. A stream is lost
// when it errs, says nothing (not even a ping) for two and a half pings (at most 12.5 s) or sends
// an event the page cannot read: the page is marked stale, waits 1 s doubling to 30 s (starting
// again only after a stream held 30 s) and reads afresh. A page that goes away closes its stream
// (`pagehide`, `freeze`) and reads at once when it is back (`pageshow`, `resume`).
import { get } from "./api.js";
import { href } from "./project.js";

const FIRST_WAIT = 1000;
const LAST_WAIT = 30000;
const HELD = 30000;

/**
 * Keep a page live. `feed(version)` is the stream's path; `read(board)` reads the page's data at
 * the board's version (it may throw, which counts as a lost round); `delta(event)` applies one
 * event; `state(live, error)` says whether the page is live. Returns a function that stops it.
 */
export function keepLive({ feed, read, delta, state }) {
  let stopped = false;
  let source = null;
  let timer = null;
  let silence = null;
  let wait = FIRST_WAIT;
  let round = 0;

  const close = () => {
    clearTimeout(silence);
    if (source) source.close();
    source = null;
  };

  const again = (after) => {
    clearTimeout(timer);
    timer = setTimeout(start, after);
  };

  const lost = (opened, error) => {
    close();
    if (stopped) return;
    state(false, error);
    if (opened && Date.now() - opened >= HELD) wait = FIRST_WAIT;
    const after = wait;
    wait = Math.min(wait * 2, LAST_WAIT);
    again(after);
  };

  async function start() {
    if (stopped) return;
    const mine = ++round;
    close();
    let board;
    try {
      board = await get("/api/board");
      await read(board);
    } catch (error) {
      if (mine === round) lost(0, error.message);
      return;
    }
    if (stopped || mine !== round) return;
    let version = board.version;
    let opened = 0;
    const quiet = (Math.min(board.ping_ms, 5000) * 5) / 2;
    const stream = new EventSource(href(feed(version)));
    source = stream;
    const heard = () => {
      clearTimeout(silence);
      silence = setTimeout(() => stream === source && lost(opened), quiet);
    };
    stream.addEventListener("open", () => {
      opened = Date.now();
      heard();
      state(true);
    });
    stream.addEventListener("ping", heard);
    stream.addEventListener("server", heard);
    stream.addEventListener("delta", (event) => {
      heard();
      let value;
      try {
        value = JSON.parse(event.data);
      } catch {
        return lost(opened);
      }
      if (typeof value?.version !== "number") return lost(opened);
      if (value.version <= version) return;
      version = value.version;
      delta(value);
    });
    stream.addEventListener("error", () => stream === source && lost(opened));
    heard();
  }

  let away = false;
  const leave = () => {
    away = true;
    clearTimeout(timer);
    round++;
    close();
  };
  const back = () => {
    if (away && !stopped) {
      away = false;
      wait = FIRST_WAIT;
      again(0);
    }
  };
  window.addEventListener("pagehide", leave);
  document.addEventListener("freeze", leave);
  window.addEventListener("pageshow", back);
  document.addEventListener("resume", back);
  start();
  return () => {
    stopped = true;
    leave();
    window.removeEventListener("pagehide", leave);
    document.removeEventListener("freeze", leave);
    window.removeEventListener("pageshow", back);
    document.removeEventListener("resume", back);
  };
}
