//! `symphony shot URL --out FILE.png`: a page as an agent sees it. A private headless Chrome
//! (crates/chrome-devtools: a profile and process of its own, leashed to this one and killed by its
//! recorded group on every exit) opens URL, waits until the page has drawn, runs the steps given in
//! their order (clicks, typing, keys, hovering, scripts, pauses, captures) and saves a PNG. It reads
//! the page only through the browser: it never starts, stops or reconfigures a Symphony server.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use symphony_args::{Flag, parse};
use symphony_chrome::{Chrome, Leash, Request};

const USAGE: &str = "usage: symphony shot [-h] URL --out FILE.png [--wait-for CSS] [--timeout SECS] \
[--size WxH | --phone] [--full-page] [--scheme light|dark] [--clock SECS] [--report] [--attach] \
[--click CSS] [--type CSS=TEXT] [--key NAME] [--hover CSS] [--eval JS] [--pause MS] [--shot FILE.png] …";
const HELP: &str =
    "A page as an agent sees it: a private headless Chrome opens URL, waits until the page
has drawn (its document loaded, a Symphony page's own navigation ready, no region still loading,
none busy on the screen (aria-busy=true, a ghost not lifted, some of its box in the window and in
each box that clips it), and no request but a feed stream in flight for 300 ms, its frames' and
its workers' among them, those Chrome runs in a process of their own too, but none of a frame
since removed, or held by a document its frame has left, nor a document its frame went on from; a
shared worker's own loads are not waited for), runs the steps in the order given, and saves a
PNG. A dialog the page or a frame opens is dismissed as it opens, and the page goes on: an alert
closed, a confirm or a prompt answered as by its Cancel, a beforeunload let leave; --report names
each. A tab the page opens (a link, a script) runs behind it as Chrome leaves it: the page is
brought back in front as soon as Chrome tells of the new one, so Chrome closes that one's
dialogs, which --report does not name. A window of its own the page opens (window.open with a
size, or popup) stays in front and keeps its dialog: one of the page's own site shares the page's
process, which its dialog holds until --timeout fails the step. It only reads the page: no server
is started, stopped or changed.

positional arguments:
  URL                  the page, an absolute URL (http://127.0.0.1:8765/symphony/tasks)

options:
  -h, --help           show this help message and exit
  --out FILE.png       where the final picture goes (its folder is made)
  --timeout SECS       how long the page's load, each step and the final capture may each take,
                       every call to the browser within them included, before the shot fails
                       naming what it waited for, a frame by its address and a worker by its
                       script's (default 20; a pause adds its own length)
  --size WxH           the viewport in CSS pixels (default 1440x900)
  --phone              a mid phone's screen instead, as the browser tests emulate it: 390x844
                       at 3x, mobile
  --full-page          capture the whole page rather than the viewport
  --scheme light|dark  the colour scheme the page is told the system prefers
  --clock SECS         the moment the page reads as now, held still: whole seconds since
                       1970-01-01 UTC, which every Date of the page and of its frames in its
                       own process says for as long as the shot lasts (its timers run on), so
                       a page that draws an age or a time is the same picture on any day; the
                       zone its times are said in stays the TZ of the environment
  --report             print, as JSON, the console errors and failed requests seen, a frame's
                       (one Chrome runs in a process of its own too) naming its address, the
                       dialogs dismissed (each one's kind, text and document), and the
                       document's scroll width against the viewport's (horizontal overflow)
  --attach             once every picture is saved, attach each (`symphony attach`): stored in
                       the main checkout's uploads and its line for a thread post printed, the
                       --shot pictures in their order, then --out's

steps, run in order before the final capture, each as often as needed:
  --wait-for CSS       wait until an element matches CSS (the page is not scrolled)
  --click CSS          scroll the first element CSS matches into view and click the middle of
                       its first line box in view that takes the press (most elements have one
                       box), once that box holds still with the element, what it holds, or its
                       own label for a control, on top there
  --type CSS=TEXT      scroll that element into view, focus it and type TEXT (CSS ends at the
                       first = outside brackets); no pointer, so no aim
  --key NAME           press a key: Tab, Enter, Escape, Space, Backspace, Delete, ArrowDown,
                       ArrowUp, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown or one
                       character, after Shift+, Control+, Alt+ or Meta+ to hold those
  --hover CSS          scroll the first element CSS matches into view and move the mouse over
                       its middle, aimed as a click is
  --eval JS            evaluate JS in the page (awaiting a promise) and print its value as JSON
  --pause MS           wait MS milliseconds
  --shot FILE.png      capture now, mid-sequence (a menu open, then closed, in one run)

A step on an element waits for it. A click or a hover aims at the middle of one of the element's
line boxes, on the whole pixel of the page nearest it (where the browser's own hit test looks
too): the first whose middle is in view and takes the press, else the first in view that no box
around it clips there, so a link wrapped over two lines is aimed at its first line, and one
taller than its scrolling box at a line the box shows. In view is in what the screen shows of the
page, where the pointer goes (its visual viewport: on a phone, a page wider than the screen or
zoomed in shows a part of its layout). It aims only once the page has settled after the scroll,
as a capture waits, and two reads in a row, each in an animation frame, find that box in one
place with its middle in view and taking the press: on top there is the element, something
inside it, or the label of the control it is (a checkbox hidden in its label or under the
label's styled track), pressed anywhere in the label but on a link, an object or another control
inside it (in a component's shadow root too, open or closed, and on what is slotted into one
there), as the label's activation clicks its control. The browser's own hit, which sees into
closed shadow roots and follows what is slotted into them, confirms a press through a label, so
such a control makes it fail as covered. Content the scroll brings near mounts and moves it
first, and can push it out of its scrolling container's view or under a sticky header: should it
leave the view, be clipped or be covered, it is scrolled to the middle again (its containers and
the page, clear of a header shorter than half the view) and the page settled again. An element
that never holds still, or stays covered (by its own ancestor too, painted over it), fails the
step (naming what covers it), and nothing is pressed. So does one that stays clipped, the browser
finding it nowhere under its middle (naming the box whose overflow or clip cuts it away there, or
whose clip-path, an inset, circle, ellipse or polygon on its border box, in pixels, percentages
or a calc() of the two, on a box nothing moves but across the page, leaves its middle out), stays
hidden (its visibility), or takes no pointer at its middle, its own ancestor on top there
(pointer-events: none, its own clip or clip-path leaving its middle out, or a cause it does not
name, as an SVG ring's unpainted middle): a press would reach the ancestor, which may not be what
was meant, so the failure names it. For pointer-events: none, which lets a press through to the
ancestor, it adds a selector that finds the ancestor alone to target instead (or says none does),
unless the ancestor is the page's body or a press there clicks another control through a label.
Once aimed, the pointer moves to that middle, and the browser's own report of the move, its
pointermove or mousemove, says where it landed and on what. Over a frame (an iframe, frame, embed
or object, in a closed shadow root too, which the browser's own hit finds there) its own document
takes the move and none reaches the page, so the pointer's place inside the frame is not confirmed:
the step goes on as aimed. Where none reaches the page over anything else (the page stopping it),
the element found there must be hovered, or where the pointer landed is unknown. Should it land
anywhere but where the aim tested, or where be unknown, or, for a click, anything but the element
(or its label) be on top there by then (a menu its hover opened; the browser's own hit confirms a
label again), the step fails naming where the pointer landed or what the press would reach, and
nothing is pressed. Typing focuses its element by script and types with no pointer, so what covers
the element takes none of it. A click, typing, a key or a hover then waits for the page to settle
again, as a capture does, first bringing the page back in front, should the step have left it
behind another tab (an Escape the page does not take, a page it opened), before asking the page
anything: a page it opened, of its own site, shares its process, and a dialog it shows in front
holds the page until another tab is in front.
Chrome is SYMPHONY_CHROME, else the first Chrome found on this machine.
Exit 0 with every picture saved (and with --attach attached), 1 when the browser, the page or a
step failed or a picture was not attached, 2 for a command line it cannot run.";

const DEFAULT_SIZE: (u32, u32) = (1440, 900);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the page's traffic must stay unchanged, with nothing but streams open, to count drawn.
const QUIET: Duration = Duration::from_millis(300);
/// How often a wait looks again.
const POLL: Duration = Duration::from_millis(25);
/// The most of a pause one observation of the browser covers.
const PAUSE_SLICE: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq)]
enum View {
    Size(u32, u32),
    Phone,
}

#[derive(Debug, PartialEq)]
enum Scheme {
    Light,
    Dark,
}

#[derive(Debug, PartialEq)]
enum Step {
    WaitFor(String),
    Click(String),
    Type(String, String),
    Key(Key),
    Hover(String),
    Eval(String),
    Pause(Duration),
    Shot(PathBuf),
}

/// One key as the browser's input takes it: its `key` and `code` names, Windows key code, the
/// text it types (none for a control key, or with Control, Alt or Meta held) and held modifiers.
#[derive(Debug, PartialEq)]
struct Key {
    key: String,
    code: String,
    key_code: u32,
    text: Option<String>,
    modifiers: u32,
}

#[derive(Debug, PartialEq)]
struct Shot {
    url: String,
    out: PathBuf,
    timeout: Duration,
    view: View,
    full_page: bool,
    scheme: Option<Scheme>,
    /// The moment the page reads as now, in seconds since 1970-01-01 UTC.
    clock: Option<u64>,
    report: bool,
    attach: bool,
    steps: Vec<Step>,
}

const FLAGS: [Flag; 17] = [
    Flag {
        name: "--out",
        value: true,
    },
    Flag {
        name: "--wait-for",
        value: true,
    },
    Flag {
        name: "--timeout",
        value: true,
    },
    Flag {
        name: "--size",
        value: true,
    },
    Flag {
        name: "--phone",
        value: false,
    },
    Flag {
        name: "--full-page",
        value: false,
    },
    Flag {
        name: "--scheme",
        value: true,
    },
    Flag {
        name: "--clock",
        value: true,
    },
    Flag {
        name: "--report",
        value: false,
    },
    Flag {
        name: "--attach",
        value: false,
    },
    Flag {
        name: "--click",
        value: true,
    },
    Flag {
        name: "--type",
        value: true,
    },
    Flag {
        name: "--key",
        value: true,
    },
    Flag {
        name: "--hover",
        value: true,
    },
    Flag {
        name: "--eval",
        value: true,
    },
    Flag {
        name: "--pause",
        value: true,
    },
    Flag {
        name: "--shot",
        value: true,
    },
];

/// `symphony shot URL --out FILE.png [OPTIONS] [STEPS]`. Its exit code.
pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let shot = match read(args) {
        Ok(Some(shot)) => shot,
        Ok(None) => {
            let _ = writeln!(out, "{USAGE}\n\n{HELP}");
            return 0;
        }
        Err(problem) => return crate::cli::refuse(err, USAGE, "shot", &problem),
    };
    let Some(binary) = symphony_chrome::find() else {
        let _ = writeln!(
            err,
            "symphony shot: no Chrome on this machine: name its executable with SYMPHONY_CHROME"
        );
        return 1;
    };
    if !Path::new(&binary).is_file() {
        let _ = writeln!(
            err,
            "symphony shot: Chrome at {binary} is not a file: name its executable with \
             SYMPHONY_CHROME"
        );
        return 1;
    }
    if let Err(problem) = run(&shot, &binary, out) {
        let _ = writeln!(err, "symphony shot: {problem}");
        return 1;
    }
    if !shot.attach {
        return 0;
    }
    match std::env::current_dir() {
        Ok(here) => super::attach::attach(&here, &shot.pictures(), out, err),
        Err(error) => {
            let _ = writeln!(err, "symphony shot: the current folder: {error}");
            1
        }
    }
}

/// The command line as a shot; `None` when it asks for help.
fn read(args: &[String]) -> Result<Option<Shot>, String> {
    let parsed = parse(args, &FLAGS)?;
    if parsed.help {
        return Ok(None);
    }
    let url = match parsed.positionals.as_slice() {
        [url] => url.clone(),
        [] => return Err("the following arguments are required: URL".into()),
        [_, rest @ ..] => return Err(format!("unrecognized arguments: {}", rest.join(" "))),
    };
    if !(url.contains("://") || url.starts_with("data:") || url.starts_with("about:")) {
        return Err(format!(
            "argument URL: {url:?} is not an absolute URL (http://127.0.0.1:8765/…)"
        ));
    }
    let mut out = None;
    let mut timeout = DEFAULT_TIMEOUT;
    let mut size = None;
    let mut phone = false;
    let mut full_page = false;
    let mut scheme = None;
    let mut clock = None;
    let mut report = false;
    let mut attach = false;
    let mut steps = Vec::new();
    for (name, value) in parsed.options() {
        match name {
            "--out" => out = Some(value),
            "--timeout" => timeout = seconds(value)?,
            "--size" => size = Some(dimensions(value)?),
            "--phone" => phone = true,
            "--full-page" => full_page = true,
            "--scheme" => {
                scheme = Some(match value {
                    "light" => Scheme::Light,
                    "dark" => Scheme::Dark,
                    _ => {
                        return Err(format!(
                            "argument --scheme: invalid choice: {value:?} (choose from 'light', 'dark')"
                        ));
                    }
                });
            }
            "--clock" => clock = Some(moment(value)?),
            "--report" => report = true,
            "--attach" => attach = true,
            "--wait-for" => steps.push(Step::WaitFor(selector(name, value)?)),
            "--click" => steps.push(Step::Click(selector(name, value)?)),
            "--hover" => steps.push(Step::Hover(selector(name, value)?)),
            "--type" => {
                let (css, text) = split_type(value)
                    .ok_or_else(|| format!("argument --type: {value:?} is not CSS=TEXT"))?;
                steps.push(Step::Type(css.to_string(), text.to_string()));
            }
            "--key" => steps.push(Step::Key(
                key(value).map_err(|why| format!("argument --key: {why}"))?,
            )),
            "--eval" => steps.push(Step::Eval(value.to_string())),
            "--pause" => steps.push(Step::Pause(Duration::from_millis(
                value.parse::<u64>().map_err(|_| {
                    format!("argument --pause: invalid MS {value:?}: whole milliseconds")
                })?,
            ))),
            "--shot" => steps.push(Step::Shot(picture(name, value)?)),
            _ => return Err(format!("unrecognized arguments: {name}")),
        }
    }
    let out = picture(
        "--out",
        out.ok_or("the following arguments are required: --out")?,
    )?;
    let view = match (size, phone) {
        (Some(_), true) => return Err("argument --phone: not allowed with argument --size".into()),
        (_, true) => View::Phone,
        (Some((width, height)), false) => View::Size(width, height),
        (None, false) => View::Size(DEFAULT_SIZE.0, DEFAULT_SIZE.1),
    };
    Ok(Some(Shot {
        url,
        out,
        timeout,
        view,
        full_page,
        scheme,
        clock,
        report,
        attach,
        steps,
    }))
}

impl Shot {
    /// Every picture the shot saves, in the order taken: each `--shot`, then `--out`.
    fn pictures(&self) -> Vec<PathBuf> {
        let shots = self.steps.iter().filter_map(|step| match step {
            Step::Shot(path) => Some(path.clone()),
            _ => None,
        });
        shots.chain([self.out.clone()]).collect()
    }
}

fn seconds(value: &str) -> Result<Duration, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0 && *seconds <= 86_400.0)
        .map(Duration::from_secs_f64)
        .ok_or_else(|| {
            format!("argument --timeout: invalid SECS {value:?}: a positive number of seconds")
        })
}

/// The seconds since 1970-01-01 UTC `--clock` names: digits alone, and few enough that the page's
/// Date holds the moment (it keeps 8.64e15 ms either side of 1970).
fn moment(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| value.bytes().all(|byte| byte.is_ascii_digit()) && *seconds <= CLOCK_MOST)
        .ok_or_else(|| {
            format!("argument --clock: invalid SECS {value:?}: whole seconds since 1970-01-01 UTC")
        })
}

/// The last second a page's Date holds.
const CLOCK_MOST: u64 = 8_640_000_000_000;

/// The script that holds a document's clock at `seconds`, run before any of its own: `Date.now()`
/// and a Date made with no argument say that moment, and every other use of Date is the browser's
/// own (a Date made from a time, `Date.parse`, `Date.UTC`, `instanceof`).
fn held_clock(seconds: u64) -> String {
    format!(
        "(() => {{ const at = {seconds} * 1000; const Real = Date; \
         function Held(...given) {{ \
         if (!new.target) return new Real(at).toString(); \
         return Reflect.construct(Real, given.length === 0 ? [at] : given, new.target); }} \
         Object.setPrototypeOf(Held, Real); Held.prototype = Real.prototype; \
         Held.now = () => at; globalThis.Date = Held; }})()"
    )
}

fn dimensions(value: &str) -> Result<(u32, u32), String> {
    let side = |side: &str| {
        side.parse::<u32>()
            .ok()
            .filter(|side| (1..=16_384).contains(side))
    };
    value
        .split_once('x')
        .and_then(|(width, height)| Some((side(width)?, side(height)?)))
        .ok_or_else(|| format!("argument --size: invalid WxH {value:?}: two whole numbers of CSS pixels, like 1440x900"))
}

fn selector(name: &str, value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err(format!("argument {name}: an empty selector"));
    }
    Ok(value.to_string())
}

fn picture(name: &str, value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.ends_with('/') {
        return Err(format!("argument {name}: {value:?} names no file"));
    }
    Ok(PathBuf::from(value))
}

/// `CSS=TEXT` at the first `=` outside brackets, parentheses and quotes, so a selector may hold
/// `[name=q]` and the text any `=`. `None` without such an `=`, or with an empty selector.
fn split_type(value: &str) -> Option<(&str, &str)> {
    let mut depth = 0_i32;
    let mut quote = None;
    for (at, character) in value.char_indices() {
        match (quote, character) {
            (Some(open), close) if open == close => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(character),
            (None, '[' | '(') => depth += 1,
            (None, ']' | ')') => depth -= 1,
            (None, '=') if depth == 0 => {
                let (css, text) = (value.get(..at)?, value.get(at + 1..)?);
                return (!css.trim().is_empty()).then_some((css, text));
            }
            _ => {}
        }
    }
    None
}

/// Modifier bits as the browser's input takes them.
const ALT: u32 = 1;
const CONTROL: u32 = 2;
const META: u32 = 4;
const SHIFT: u32 = 8;

/// A key by its name, with any `Shift+`, `Control+`, `Alt+` or `Meta+` before it.
fn key(name: &str) -> Result<Key, String> {
    let mut modifiers = 0;
    let mut rest = name;
    // A lone `+`, or one after the last modifier (`Shift++`), is the plus key itself.
    while let Some((modifier, after)) = rest
        .split_once('+')
        .filter(|(modifier, after)| !modifier.is_empty() && !after.is_empty())
    {
        modifiers |= match modifier {
            "Alt" | "Option" => ALT,
            "Control" | "Ctrl" => CONTROL,
            "Meta" | "Cmd" | "Command" => META,
            "Shift" => SHIFT,
            other => {
                return Err(format!(
                    "{other:?} in {name:?} is no modifier: Shift, Control, Alt or Meta"
                ));
            }
        };
        rest = after;
    }
    let named = |key: &str, key_code, text: Option<&str>| {
        (
            key.to_string(),
            key.to_string(),
            key_code,
            text.map(str::to_string),
        )
    };
    let (key, code, key_code, text) = match rest {
        "Tab" => named("Tab", 9, None),
        "Enter" | "Return" => named("Enter", 13, Some("\r")),
        "Escape" | "Esc" => named("Escape", 27, None),
        "Backspace" => named("Backspace", 8, None),
        "Delete" => named("Delete", 46, None),
        "Space" | " " => (" ".into(), "Space".into(), 32, Some(" ".into())),
        "ArrowUp" => named("ArrowUp", 38, None),
        "ArrowDown" => named("ArrowDown", 40, None),
        "ArrowLeft" => named("ArrowLeft", 37, None),
        "ArrowRight" => named("ArrowRight", 39, None),
        "Home" => named("Home", 36, None),
        "End" => named("End", 35, None),
        "PageUp" => named("PageUp", 33, None),
        "PageDown" => named("PageDown", 34, None),
        one if one.chars().count() == 1 => {
            let character = one.chars().next().unwrap_or_default();
            let typed = if modifiers & SHIFT != 0 {
                character.to_uppercase().collect()
            } else {
                character.to_string()
            };
            let upper = character.to_ascii_uppercase();
            let (code, key_code) = if character.is_ascii_alphabetic() {
                (format!("Key{upper}"), u32::from(upper))
            } else if character.is_ascii_digit() {
                (format!("Digit{character}"), u32::from(character))
            } else {
                (String::new(), 0)
            };
            (typed.clone(), code, key_code, Some(typed))
        }
        _ => {
            return Err(format!(
                "{rest:?} is no key: Tab, Enter, Escape, Space, Backspace, Delete, ArrowDown, \
                 ArrowUp, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown or one character"
            ));
        }
    };
    // A held Control, Alt or Meta makes a shortcut, which types nothing.
    let text = text.filter(|_| modifiers & (ALT | CONTROL | META) == 0);
    Ok(Key {
        key,
        code,
        key_code,
        text,
        modifiers,
    })
}

/// One wait of the shot: `--timeout` from when it began. The driver holds its end too, so every
/// call to the browser within it, however stuck, ends by then.
#[derive(Clone, Copy)]
struct Wait {
    until: Instant,
    timeout: Duration,
}

impl Wait {
    /// A wait of `timeout` from now, and `extra` beyond it (a pause's own length).
    fn begin(chrome: &mut Chrome, timeout: Duration, extra: Duration) -> Wait {
        let until = Instant::now() + timeout + extra;
        chrome.set_deadline(Some(until));
        Wait { until, timeout }
    }

    fn over(self) -> bool {
        Instant::now() >= self.until
    }

    /// Its length, as a failure names it.
    fn said(self) -> String {
        seconds_said(self.timeout)
    }
}

/// A driver failure as the shot says it: `late()` once the wait has run out, else what failed,
/// after `after`.
fn said<T>(
    result: symphony_chrome::Result<T>,
    after: &str,
    late: impl FnOnce() -> String,
) -> Result<T, String> {
    result.map_err(|error| {
        if error.past_deadline() {
            late()
        } else {
            format!("{after}: {error}")
        }
    })
}

fn run(shot: &Shot, binary: &str, out: &mut dyn Write) -> Result<(), String> {
    let program = std::env::current_exe()
        .map_err(|error| format!("this program's own path, to leash Chrome: {error}"))?;
    let mut chrome = Chrome::launch(binary, &Leash::new(program, &[]).quiet())
        .map_err(|error| error.to_string())?;
    // The page's load: its view, its navigation and its drawing, within one wait.
    let wait = Wait::begin(&mut chrome, shot.timeout, Duration::ZERO);
    prepare(&mut chrome, shot, wait)?;
    chrome
        .navigate(&shot.url, shot.timeout)
        .map_err(|error| not_loaded(&shot.url, wait, error.past_deadline(), &error.to_string()))?;
    settle(&mut chrome, wait, &shot.url)?;
    for step in &shot.steps {
        let extra = match step {
            Step::Pause(length) => *length,
            _ => Duration::ZERO,
        };
        let wait = Wait::begin(&mut chrome, shot.timeout, extra);
        step.run(&mut chrome, shot, wait, out)?;
    }
    let wait = Wait::begin(&mut chrome, shot.timeout, Duration::ZERO);
    settle(&mut chrome, wait, "the final capture")?;
    capture(&mut chrome, &shot.out, shot.full_page, wait)?;
    if shot.report {
        let wait = Wait::begin(&mut chrome, shot.timeout, Duration::ZERO);
        let late = || format!("the report: no answer within {}", wait.said());
        let requests = said(chrome.requests(), "the report", late)?;
        let layout = said(chrome.eval(LAYOUT), "the report", late)?;
        let said = report(chrome.events(), &requests, &layout);
        writeln!(out, "{said}").map_err(|error| format!("printing the report: {error}"))?;
    }
    Ok(())
}

/// Why the page did not load: the wait ran out (`late`), or Chrome's reason `why` (a refused
/// connection, an unknown host), the URL named once.
fn not_loaded(url: &str, wait: Wait, late: bool, why: &str) -> String {
    if late {
        return format!("{url} did not load within {}", wait.said());
    }
    let why = why.strip_prefix(&format!("{url}: ")).unwrap_or(why);
    format!("{url} did not load: {why}")
}

/// The viewport, the colour scheme, the clock held and, for a report, the console's domains, before
/// the page loads.
fn prepare(chrome: &mut Chrome, shot: &Shot, wait: Wait) -> Result<(), String> {
    let late = || format!("the browser did not answer within {}", wait.said());
    let view = match shot.view {
        View::Phone => chrome.phone_screen(),
        View::Size(width, height) => chrome
            .call(
                "Emulation.setDeviceMetricsOverride",
                json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
            )
            .map(drop),
    };
    said(view, "setting the view", late)?;
    // A frame in a process of its own (a cross-site one, a sandboxed one: a document's mockup) is
    // answered in that process, and a worker's script in the worker: followed, their requests end
    // as the page's own do.
    said(chrome.follow_frames(), "following the page's frames", late)?;
    // A dialog holds its document until answered: each is dismissed as it opens, and the report
    // names it.
    chrome.dismiss_dialogs();
    // A tab the page opens runs behind it, as Chrome leaves it, and Chrome closes its dialogs
    // there. In front, it would leave the page drawing no frames, and one of the page's own site
    // shares its renderer, which that tab's dialog would hold, and every call on the page with it,
    // the click that opened it among them. A window of its own stays in front in its own right
    // and keeps its dialog, so such a call waits until the step's --timeout.
    said(chrome.keep_in_front(), "keeping the page in front", late)?;
    if let Some(scheme) = &shot.scheme {
        let value = match scheme {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        };
        said(
            chrome.call(
                "Emulation.setEmulatedMedia",
                json!({"features": [{"name": "prefers-color-scheme", "value": value}]}),
            ),
            "setting the colour scheme",
            late,
        )?;
    }
    if let Some(seconds) = shot.clock {
        said(
            chrome.call(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source": held_clock(seconds)}),
            ),
            "holding the page's clock",
            late,
        )?;
    }
    if shot.report {
        for domain in CONSOLE {
            said(chrome.call(domain, json!({})), domain, late)?;
        }
        // A frame in a process of its own has a console of its own.
        chrome.enable_in_frames(&CONSOLE);
    }
    Ok(())
}

/// The domains a report reads the console's errors from, the page's own and each frame's.
const CONSOLE: [&str; 2] = ["Runtime.enable", "Log.enable"];

impl Step {
    fn run(
        &self,
        chrome: &mut Chrome,
        shot: &Shot,
        wait: Wait,
        out: &mut dyn Write,
    ) -> Result<(), String> {
        match self {
            Step::WaitFor(css) => {
                locate(chrome, css, wait, false).map_err(|why| format!("--wait-for {why}"))?;
            }
            Step::Click(css) | Step::Hover(css) => {
                let clicking = matches!(self, Step::Click(_));
                let flag = if clicking { "--click" } else { "--hover" };
                let press = aim(chrome, css, wait, flag)?;
                let step = format!("{flag} {css}");
                let late = || format!("{step}: the browser did not answer within {}", wait.said());
                // The pointer goes to the point aimed at, the page listening for where it lands,
                // and only once it is there, on the element, is anything pressed.
                said(chrome.eval(WATCH), &step, late)?;
                said(move_to(chrome, press.screen), &step, late)?;
                let arrived = said(chrome.eval(&arrival(css, press.page)), &step, late)?;
                let mut arrival = Arrival::read(&arrived);
                // No report, and nothing the page sees there hovered: the browser's own hit, which
                // sees into a closed shadow root, says whether a frame took the move there.
                if let (true, Some(point)) = (arrival.unseen(), arrival.document) {
                    arrival.framed = said(frame_at(chrome, point), &step, late)?;
                }
                // A click the page takes through a label, there now, the browser's own hit
                // confirms, as it did for the aim: what its hover put there the page may not see.
                if let (true, Reached::Label, Some(point)) =
                    (clicking, &arrival.hit, arrival.document)
                {
                    arrival.hit =
                        Reached::confirmed(said(confirm(chrome, css, point), &step, late)?);
                }
                if let Some(why) = arrival.refused(press.page, clicking) {
                    return Err(format!("{step}{why}"));
                }
                if clicking {
                    said(press_at(chrome, press.screen), &step, late)?;
                }
                settle(chrome, wait, &step)?;
            }
            Step::Type(css, text) => {
                locate(chrome, css, wait, false).map_err(|why| format!("--type {why}"))?;
                let step = format!("--type {css}");
                let late = || format!("{step}: the browser did not answer within {}", wait.said());
                let focused = said(
                    chrome.eval(&format!(
                        "(() => {{ const element = document.querySelector({}); element.scrollIntoView({{block: 'center', inline: 'center'}}); element.focus(); return document.activeElement === element; }})()",
                        json!(css)
                    )),
                    &step,
                    late,
                )?;
                if focused != json!(true) {
                    return Err(format!("{step}: the element cannot take focus"));
                }
                said(
                    chrome.call("Input.insertText", json!({"text": text})),
                    &step,
                    late,
                )?;
                settle(chrome, wait, &step)?;
            }
            Step::Key(key) => {
                let step = format!("--key {}", key.key);
                said(press(chrome, key), &step, || {
                    format!("{step}: the browser did not answer within {}", wait.said())
                })?;
                settle(chrome, wait, &step)?;
            }
            Step::Eval(script) => {
                let value = said(chrome.eval(script), "--eval", || {
                    format!("--eval: no answer within {}", wait.said())
                })?;
                writeln!(out, "{value}").map_err(|error| format!("--eval: printing: {error}"))?;
            }
            Step::Pause(length) => {
                let until = Instant::now() + *length;
                while let Some(left) = until
                    .checked_duration_since(Instant::now())
                    .filter(|left| !left.is_zero())
                {
                    said(chrome.pump(left.min(PAUSE_SLICE)), "--pause", || {
                        format!("--pause: the browser did not answer within {}", wait.said())
                    })?;
                }
            }
            Step::Shot(path) => {
                settle(chrome, wait, &format!("--shot {}", path.display()))?;
                capture(chrome, path, shot.full_page, wait)?;
            }
        }
        Ok(())
    }
}

/// How a probe reads an element: where it is, after scrolling it to the middle of the view, or in
/// the next animation frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    Now,
    Scrolled,
    NextFrame,
}

/// What a [`probe`] aims at, `box`, and what is on top at its middle, where the pointer would
/// land, `hit`, what is under a point [`REACH`]ed as Chrome's pointer finds it.
///
/// What clips the element is `clips`: its own `clip`, then each box around it that can clip it
/// (the boxes its containing blocks are in, so an absolute or a fixed one escapes the boxes it
/// does not lie in) by its `clip`, its overflow other than `visible` on an axis (a scrolling box
/// among them) or its paint containment, to the box's padding edge. The page and its body are the
/// view's.
///
/// In view is in the visual viewport, `vv`, what the screen shows of the page: its edges in the
/// coordinates of the page's client rects and hit tests, the layout viewport's, which on a phone
/// can be far larger (a page wider than the screen widens it, to a quarter's zoom) and is scrolled
/// apart from it, as one zoomed in is. Without the visual viewport's interface, the window is it.
///
/// `box` is the first of the element's line boxes (its client rects with an area) whose middle is
/// in view and takes the press there, else the first in view clipped by nothing there, else the
/// first in view, else its first, else its bounding box: a link wrapped over two lines is aimed at
/// its first line, not at the middle of its bounding box, which may hold none of it, and one taller
/// than the view, or than the scrolling box it is in, at a line that box shows.
///
/// A box's middle (`middle`) is the point nearest its middle that lies on a whole CSS pixel of the
/// document (`whole`: the page's coordinates moved by its scroll), inside the box where one does
/// (any box a pixel wide or more), so the page's hit test, the browser's own ([`confirm`], which
/// takes whole pixels) and the press all take one point.
///
/// `hit`, at the box's middle, is `true` when the element takes the press: what is there is the
/// element or inside it. It is `{label}` when what is there is a label whose control the element
/// is (`label.control`, by `for` or by holding it) and nothing interactive lies between, as far as
/// the page can see, as the label's activation then clicks it (a checkbox hidden in its label or
/// under the label's styled track), for the browser's own hit to confirm. Else it is why not, the
/// first that holds, each cause named only where the browser or the geometry backs it:
/// - `{hidden}` for an element whose visibility is not `visible`;
/// - `{clipped}` for the box around it that clips it away there, the browser listing the element
///   nowhere under the point (`listed`: `document.elementsFromPoint` lists every element a press
///   there could reach, down to the page, so what is clipped away there is left out);
/// - a cover's name for what is there and does not hold the element, or holds it but is painted
///   over it, the element listed under it (the element in a shadow root there named in its host's,
///   `inside`: `button#inner in x-pressable#host's shadow root`);
/// - `{clipped}` for the element between it and its ancestor on top there whose `clip-path` leaves
///   the point out (`excludes`: an inset, circle, ellipse or polygon, its border box the reference,
///   its lengths in pixels, percentages or the `calc()` of the two Chrome computes them to, read
///   by `size` from the arguments `parts` splits outside parentheses, on a box that neither it nor
///   anything around it up the flat tree moves but across the page (`level`: no `transform` but a
///   translation in the page's plane, which the box's place already carries, `translate` with no
///   depth, and no `scale`, `rotate`, `offset-path` or `zoom`, so the point is tested in the box's
///   own coordinates, never a mirrored or turned ancestor's); any other shape, a rounded inset's
///   corners and an SVG element's clip-path are never claimed), none when the element has
///   `pointer-events: none`;
/// - `{holder, why, target, page, labels}` for its own ancestor there, which a press there reaches
///   instead: `why` is `clip` for the element's own `clip`, or its own `clip-path` leaving the
///   point out, `none` for its `pointer-events: none`, else `unnamed`, no cause established (an SVG
///   ring's unpainted middle among them); `target` is a selector that finds the ancestor alone
///   (offered only for `none`, where a press on the ancestor is what the element lets through),
///   `page` whether the ancestor is the page's body or root, no target, and `labels` the control a
///   press there clicks through a label, where the page sees the whole way to it.
///
/// `null` is nothing there (a point outside the page).
///
/// An element is named as [`NAMES`] says; `alone` gives a selector that finds it alone, its name
/// or a path of names by `:nth-of-type` from it up, its own place among its kin always after its
/// parent's name (`nav.tools > button:nth-of-type(2)`), or `null` for an element in a shadow root,
/// or none found.
const AIM: &str = r#"
const finds = (css, e) => { try { const all = document.querySelectorAll(css); return all.length === 1 && all[0] === e; } catch (error) { return false; } };
const alone = e => {
  if (e.getRootNode() !== document) return null;
  if (finds(name(e), e)) return name(e);
  let css = '';
  for (let n = e; n; n = n.parentElement) {
    const kin = n.parentElement ? [...n.parentElement.children].filter(s => s.localName === n.localName) : [n];
    css = name(n) + (kin.length > 1 ? ':nth-of-type(' + (kin.indexOf(n) + 1) + ')' : '') + (css && ' > ' + css);
    if ((n !== e || kin.length === 1) && finds(css, e)) return css;
  }
  return null;
};
const style = getComputedStyle(element);
const cuts = (e, t) => {
  if (!/absolute|fixed/.test(t.position) || !t.clip.startsWith('rect(')) return null;
  const b = e.getBoundingClientRect();
  const [top, right, bottom, left] = t.clip.slice(5, -1).split(/[\s,]+/).map(side => side === 'auto' ? null : parseFloat(side));
  return {x: true, y: true, edges: [b.x + (left ?? 0), b.y + (top ?? 0), b.x + (right ?? b.width), b.y + (bottom ?? b.height)]};
};
const confines = t => t.transform !== 'none' || t.perspective !== 'none' || t.filter !== 'none' || /layout|paint|strict|content/.test(t.contain) || /transform|perspective|filter/.test(t.willChange) || (t.containerType || 'normal') !== 'normal';
const clips = [];
{
  const self = cuts(element, style);
  if (self) clips.push({by: null, ...self});
  let position = style.position;
  for (let a = flat(element); a && a !== document.body && a !== document.documentElement; a = flat(a)) {
    const t = getComputedStyle(a);
    if (t.display === 'contents' || (a instanceof SVGElement && a.ownerSVGElement)) continue;
    if ((position === 'fixed' && !confines(t)) || (position === 'absolute' && t.position === 'static' && !confines(t))) continue;
    position = t.position;
    const cut = cuts(a, t);
    if (cut) clips.push({by: a, ...cut});
    const paint = /paint|strict|content/.test(t.contain);
    const x = paint || t.overflowX !== 'visible', y = paint || t.overflowY !== 'visible';
    if ((x || y) && t.display !== 'inline') {
      const b = a.getBoundingClientRect(), left = b.x + a.clientLeft, top = b.y + a.clientTop;
      clips.push({by: a, x, y, edges: [left, top, left + a.clientWidth, top + a.clientHeight]});
    }
  }
}
const away = (c, x, y) => (c.x && (x < c.edges[0] || x >= c.edges[2])) || (c.y && (y < c.edges[1] || y >= c.edges[3]));
const size = (v, basis) => { const c = /^calc\((-?\d*\.?\d+)% ([+-]) (\d*\.?\d+)px\)$/.exec(v); return c ? parseFloat(c[1]) / 100 * basis + parseFloat(c[2] + c[3]) : /^-?\d*\.?\d+px$/.test(v) ? parseFloat(v) : /^-?\d*\.?\d+%$/.test(v) ? parseFloat(v) / 100 * basis : NaN; };
const parts = v => v.match(/(?:[^\s(]+|\([^()]*\))+/g) ?? [];
const moved = t => !/^(none|matrix\(1, 0, 0, 1, [^,]+, [^,]+\))$/.test(t.transform) || (t.translate || 'none').split(' ').length > 2 || ['scale', 'rotate', 'offsetPath'].some(p => (t[p] || 'none') !== 'none') || (t.zoom || '1') !== '1';
const level = e => { for (let a = e; a; a = flat(a)) if (moved(getComputedStyle(a))) return false; return true; };
const excludes = (e, x, y) => {
  const t = e instanceof HTMLElement ? getComputedStyle(e) : null;
  const m = t && t.display !== 'inline' && level(e) ? /^(inset|circle|ellipse|polygon)\((.*)\)( border-box)?$/.exec(t.clipPath) : null;
  const b = e.getBoundingClientRect();
  if (!m || Math.abs(b.width - e.offsetWidth) > 1 || Math.abs(b.height - e.offsetHeight) > 1) return false;
  const [w, h, px, py] = [b.width, b.height, x - b.x, y - b.y];
  if (m[1] === 'polygon') {
    const pairs = m[2].split(', ');
    const rule = /^(evenodd|nonzero)$/.test(pairs[0]) ? pairs.shift() : 'nonzero';
    const points = pairs.map(parts).map(([a, c, more]) => more === undefined ? [size(a, w), size(c, h)] : [NaN, NaN]);
    if (points.length < 3 || !points.flat().every(Number.isFinite)) return false;
    let crossings = 0, winding = 0;
    points.forEach(([x1, y1], i) => { const [x2, y2] = points[(i + 1) % points.length]; if ((y1 <= py) !== (y2 <= py) && x1 + (py - y1) / (y2 - y1) * (x2 - x1) > px) { crossings++; winding += y2 > y1 ? 1 : -1; } });
    return rule === 'evenodd' ? crossings % 2 === 0 : winding === 0;
  }
  if (m[1] === 'inset') {
    const sides = parts(m[2].split(' round ')[0]);
    if (sides.length > 4) return false;
    const [top, right = top, bottom = top, left = right] = sides;
    const [l, r, t0, b0] = [size(left, w), w - size(right, w), size(top, h), h - size(bottom, h)];
    return r <= l || b0 <= t0 || px < l || px >= r || py < t0 || py >= b0;
  }
  const [form, at] = m[2].split(/(?:^| )at /);
  const words = form ? parts(form) : [];
  const [cx, cy] = at ? parts(at).map((v, i) => size(v, i ? h : w)) : [w / 2, h / 2];
  const radius = (v, basis, sides) => v === undefined || v === 'closest-side' ? Math.min(...sides) : v === 'farthest-side' ? Math.max(...sides) : size(v, basis);
  if (m[1] === 'circle' && words.length <= 1) return Math.hypot(px - cx, py - cy) > radius(words[0], Math.hypot(w, h) / Math.SQRT2, [cx, w - cx, cy, h - cy]);
  if (m[1] === 'ellipse' && words.length !== 1 && words.length <= 2) return ((px - cx) / radius(words[0], w, [cx, w - cx])) ** 2 + ((py - cy) / radius(words[1], h, [cy, h - cy])) ** 2 > 1;
  return false;
};
const whole = (from, size, scroll) => { const start = from + scroll, end = start + size, near = Math.round(start + size / 2); return (near >= start && near < end ? near : Math.ceil(start) < end ? Math.ceil(start) : near) - scroll; };
const middle = r => [whole(r.x, r.width, scrollX), whole(r.y, r.height, scrollY)];
const vv = window.visualViewport ?? {offsetLeft: 0, offsetTop: 0, width: innerWidth, height: innerHeight};
const seen = ([x, y]) => x >= vv.offsetLeft && x < vv.offsetLeft + vv.width && y >= vv.offsetTop && y < vv.offsetTop + vv.height;
const rects = [...element.getClientRects()].filter(r => r.width > 0 && r.height > 0);
const viewed = rects.filter(r => seen(middle(r)));
const box = viewed.find(r => takes(reach(...middle(r)))) ?? viewed.find(r => !clips.some(c => away(c, ...middle(r)))) ?? viewed[0] ?? rects[0] ?? element.getBoundingClientRect();
const [x, y] = middle(box);
const p = reach(x, y);
const listed = document.elementsFromPoint(x, y).some(e => element.contains(e));
const clipper = listed ? null : clips.find(c => c.by && away(c, x, y));
let held = false;
for (let e = element; e && !held; e = flat(e)) held = e === p.deep;
let shaped = null;
for (let e = element; held && !listed && style.pointerEvents !== 'none' && e && e !== p.deep && !shaped; e = flat(e)) if (excludes(e, x, y)) shaped = e;
const page = p.at === document.body || p.at === document.documentElement;
const hit = !p.at ? null
  : takes(p) ? element.contains(p.at) || {label: true}
  : style.visibility !== 'visible' ? {hidden: style.visibility}
  : clipper ? {clipped: named(clipper.by)}
  : !held || listed ? inside(p.deep)
  : shaped && shaped !== element ? {clipped: inside(shaped)}
  : {holder: named(p.at), why: shaped || clips.some(c => !c.by && away(c, x, y)) ? 'clip' : style.pointerEvents === 'none' ? 'none' : 'unnamed', target: page ? null : alone(p.at), page, labels: p.via && !p.blind && (alone(p.via) ?? inside(p.via))};
"#;

/// How a [`probe`] and [`confirm`] name an element: as a selector names it, its tag, id and first
/// three classes, after its parent's when it has neither id nor class (`main > div`), which need
/// not find it alone; `inside`, one in a shadow root in its outermost host's (`button#inner in
/// x-pressable#host's shadow root`). And how they walk up: `up` to the parent or the shadow root's
/// host, `flat` up the flat tree, as a press travels (a slotted node's slot first).
const NAMES: &str = r#"
const name = e => e.localName + (e.id ? '#' + CSS.escape(e.id) : '') + [...e.classList].slice(0, 3).map(c => '.' + CSS.escape(c)).join('');
const named = e => (!e.id && !e.classList.length && e.parentElement ? name(e.parentElement) + ' > ' : '') + name(e);
const up = e => e.parentElement || (e.parentNode instanceof ShadowRoot ? e.parentNode.host : null);
const flat = e => e.assignedSlot || up(e);
const inside = e => { let host = e; while (host.getRootNode() instanceof ShadowRoot) host = host.getRootNode().host; return host === e ? named(e) : named(e) + ' in ' + named(host) + "'s shadow root"; };
"#;

/// How a [`probe`] and [`ARRIVED`] tell what a press reaches, given `element`, the element aimed at,
/// and [`INTERACTIVE`].
///
/// What is under a point is `reach`ed as Chrome's pointer finds it: the document's element there
/// (`at`, a shadow host for what its shadow root draws, as a selector can name), then through every
/// open shadow root to the element really there (`deep`), and that pair is `reached`: the first
/// interactive element ([`INTERACTIVE`]) from `deep` up the flat tree, as the press travels (a
/// slotted node's slot first, a shadow root's host), as a label's activation looks for one: when it
/// is a label, `via` is the control it clicks. The page cannot see into a closed shadow root, where
/// a control may take the press or what is slotted there may lie in one, nor tell a closed root on
/// a built-in element, such as a span, from no root: so a press the page takes as the label's is
/// confirmed with the browser's own hit, which sees into closed roots and follows their slots
/// ([`confirm`]). `blind` says the walk passed a custom element without an open shadow root, one
/// the page knows may hide one: no label's control is then named as what a press there clicks. The
/// element `takes` the press when what is there is it, inside it, or a label whose activation
/// clicks it.
const REACH: &str = r#"
const unseen = e => e instanceof HTMLElement && e.localName.includes('-') && !e.shadowRoot;
const reach = (x, y) => {
  const at = document.elementFromPoint(x, y);
  let deep = at;
  for (let depth = 0; deep && deep.shadowRoot && depth < 32; depth++) {
    const inner = deep.shadowRoot.elementFromPoint(x, y);
    if (!inner || inner === deep) break;
    deep = inner;
  }
  return reached(at, deep);
};
const reached = (at, deep) => {
  let first = deep, blind = false;
  for (; first && !first.matches(INTERACTIVE); first = flat(first)) blind ||= unseen(first);
  return {at, deep, via: first && first.localName === 'label' ? first.control : null, blind};
};
const takes = p => !!p.at && (element.contains(p.at) || p.via === element);
"#;

/// Set before the pointer moves: a watch for the move's own report, kept on the window under one
/// name the page does not list (not enumerable), until [`ARRIVED`] reads and removes it. The report
/// is the first `pointermove` or `mousemove` the browser sent (`isTrusted`, not one a page script
/// made) that reaches the window, in the capture phase, where the window hears an event first: the
/// pointer's point in the page's coordinates (the event's `clientX` and `clientY`, a mouse event's
/// in whole pixels) and what it is over (the event's target, the browser's own hit where the
/// pointer really is: through open shadow roots, a closed root's host for what that root holds;
/// `null` for anything but an element). Chrome sends both on every move, to the same point too.
/// The watch listens through `EventTarget`'s own methods, as they were when it began, never the
/// window's, which a page may replace; it stops listening once a report came or once read. A
/// frame's document takes the move over a frame, a page listening first can stop it, and a page
/// whose `EventTarget` methods throw lets the watch hear nothing: then no report is kept, and
/// nothing the watch does throws into the step or the page.
const WATCH: &str = "(() => { try { const key = 'symphony-shot-pointer'; \
try { window[key]?.stop(); } catch {} \
const {addEventListener: listen, removeEventListener: unlisten} = EventTarget.prototype; \
const kinds = ['pointermove', 'mousemove']; \
const watch = {report: null, stop: () => { for (const kind of kinds) unlisten.call(window, kind, take, true); }}; \
const take = event => { try { if (!event.isTrusted) return; const node = event.composedPath()[0]; \
watch.report = {x: event.clientX, y: event.clientY, at: node instanceof Element ? node : null}; \
watch.stop(); } catch {} }; \
Object.defineProperty(window, key, {value: watch, configurable: true, writable: true}); \
for (const kind of kinds) listen.call(window, kind, take, true); \
return true; } catch { return false; } })()";

/// The elements whose own document takes the pointer over them: a frame of any kind.
const FRAMES: &str = "iframe, frame, embed, object";

/// What the pointer reached once moved to the point aimed at, `[x, y]` in the page's coordinates,
/// for the element aimed at, `element` (`null` once it has left the page): the move's report as
/// [`WATCH`] kept it, the pointer's point and what it is over, or `null` when none came (the watch
/// removed from the page either way); what the page's hit test finds at the point aimed at, now;
/// whether the element found there is hovered, which says the pointer is over it where no report
/// came; whether it is a frame ([`FRAMES`]), whose own document takes the move, unheard by the
/// page, and hovers its element only when it shares the page's process; and the point aimed at in
/// the document's coordinates, where the browser's own hit looks for a frame the page cannot see
/// ([`frame_at`]). What is over or found there is `true` when it is the element or inside it,
/// `{label}` when it is a label whose activation clicks the element as far as the page can see
/// ([`REACH`]), for the browser's own hit to confirm before a click ([`confirm`]), else its name,
/// or `null` for no element. With no report yet it waits up to two animation frames for one, in
/// case the browser dispatches the move with the next frame; that also gives a page the move
/// replaced (its hover loading another, which has no watch) the frames it takes to hover what is
/// under the pointer.
const ARRIVED: &str = r#"
const key = 'symphony-shot-pointer';
for (let frame = 0; frame < 2 && !window[key]?.report; frame++) await new Promise(resolve => requestAnimationFrame(resolve));
const watch = window[key];
delete window[key];
try { watch?.stop(); } catch {}
const record = watch ? watch.report : null;
const outer = e => { let host = e; while (host.getRootNode() instanceof ShadowRoot) host = host.getRootNode().host; return host; };
const hit = reach(x, y);
const said = p => !p.at ? null : element && takes(p) ? element.contains(p.at) || {label: true} : inside(p.deep);
return {gone: !element || !element.isConnected,
  pointer: record ? {x: record.x, y: record.y, on: record.at ? said(reached(outer(record.at), record.at)) : null} : null,
  hit: said(hit), hovered: !!hit.deep && hit.deep.matches(':hover'), framed: !!hit.deep && hit.deep.matches(FRAMES),
  document: [x + scrollX, y + scrollY]};
"#;

/// A script reading what the pointer reached once moved to `(x, y)` in the page's coordinates, for
/// the first element `css` matches ([`ARRIVED`]).
fn arrival(css: &str, (x, y): (f64, f64)) -> String {
    format!(
        "(async () => {{ const element = document.querySelector({}); const [x, y] = [{}, {}]; \
         const INTERACTIVE = {}; const FRAMES = {}; {NAMES}{REACH}{ARRIVED} }})()",
        json!(css),
        json!(x),
        json!(y),
        json!(INTERACTIVE),
        json!(FRAMES)
    )
}

/// The function [`confirm`] calls on a node on the press's way up the flat tree (`this`, inside a
/// closed shadow root too), with the selector of the element aimed at, [`INTERACTIVE`], the node
/// the browser itself found at the point (`hit`) and the node whose slot the browser has said it
/// has none of (`known`, or `null`): `true` when, up the flat tree from `this`, the element comes,
/// or a label whose control the element is, before anything else interactive; else the name of
/// the element found there (a text node's by the element it lies in). The page cannot see the slot
/// a node is assigned in a closed shadow root, so the walk stops at a node with no slot it can see
/// whose parent could hold a closed root (an HTML element with no open one, a custom element or
/// one of those a shadow root can be attached to) and answers that node itself, for the browser to
/// say its slot.
const CONFIRM: &str = r#"
const element = document.querySelector(css);
const found = hit.nodeType === 1 ? hit : flat(hit);
const hosts = /^(article|aside|blockquote|body|div|footer|h[1-6]|header|main|nav|p|section|span)$|-/;
for (let e = this; e; e = flat(e)) {
  if (e === element) return true;
  if (e.nodeType === 1 && e.matches(interactive)) return e.localName === 'label' && e.control === element ? true : inside(found);
  const p = e.parentElement;
  if (e !== known && !e.assignedSlot && p instanceof HTMLElement && !p.shadowRoot && hosts.test(p.localName)) return e;
}
return found ? inside(found) : null;
"#;

/// What, pressed inside a label, takes the press itself rather than activating the label (HTML's
/// interactive content, and an object, whose own document takes a press as an iframe's does): a
/// press on a link or another control inside a label, a component's shadow root among the places
/// it lies and a node slotted into one among the places pressed, never reaches the label's
/// control.
const INTERACTIVE: &str = "a[href], audio[controls], button, details, embed, iframe, img[usemap], input, label, object, select, textarea, video[controls]";

/// A script reading the box a click or a hover aims at in the first element `css` matches, the
/// visual viewport's edges and how far the page is scrolled, and what is on top at the box's middle
/// ([`AIM`]), as `look` says: `null` while nothing matches, `{invalid}` for what is no selector.
fn probe(css: &str, look: Look) -> String {
    let read = format!(
        "(() => {{ let element; try {{ element = document.querySelector({}); }} catch (error) {{ return {{invalid: String(error.message)}}; }} \
         if (!element) return null; {} \
         const INTERACTIVE = {}; {NAMES}{REACH}{AIM} \
         return {{x: box.x, y: box.y, width: box.width, height: box.height, point: [x, y], view: [vv.offsetLeft, vv.offsetTop, vv.width, vv.height], scroll: [scrollX, scrollY], hit}}; }})()",
        json!(css),
        if look == Look::Scrolled {
            "element.scrollIntoView({block: 'center', inline: 'center'});"
        } else {
            ""
        },
        json!(INTERACTIVE)
    );
    if look == Look::NextFrame {
        format!("new Promise(resolve => requestAnimationFrame(() => resolve({read})))")
    } else {
        read
    }
}

/// Whether the element takes a press at the middle of its box, as a [`probe`] found it.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    /// What is there is the element itself, something inside it, or a label whose activation
    /// clicks it, the browser's own hit agreeing ([`AIM`], [`confirm`]).
    Target,
    /// What is there is a label whose activation clicks it, as far as the page can see: the
    /// browser's own hit is still to confirm it ([`confirm`]).
    Label,
    /// Why not.
    Missed(Miss),
}

/// Why an element does not take a press at the middle of its box, the first that holds ([`AIM`]).
#[derive(Clone, Debug, PartialEq)]
enum Miss {
    /// Its visibility is not `visible` (it is `hidden` or `collapse`), so no press reaches it.
    Hidden(String),
    /// A box around it, by its name, clips it away there (its overflow, its paint containment, its
    /// `clip` or its `clip-path`): it is scrolled out of its scrolling box's view, or folded or cut
    /// away.
    Clipped(String),
    /// Another element, by its name, on top there: one that does not hold it, or one that does but
    /// is painted over it.
    Covered(String),
    /// Its own ancestor, which a press there reaches instead.
    Holder(Holder),
    /// No element: the point is outside the page (under a scrollbar, past an edge).
    Nothing,
}

/// The element's own ancestor on top at its middle, which a press there reaches instead.
#[derive(Clone, Debug, PartialEq)]
struct Holder {
    /// Its name ([`AIM`]).
    name: String,
    why: Why,
    /// What to target in the element's place.
    instead: Instead,
    /// The control a press there clicks through a label (the holder's or one around it), by a
    /// selector that finds it alone, else its name (in its host's, from a shadow root): no
    /// substitute for the element.
    labels: Option<String>,
}

/// What a failure offers to target in place of an element whose own ancestor takes the press.
#[derive(Clone, Debug, PartialEq)]
enum Instead {
    /// The ancestor, by a selector that finds it alone.
    Alone(String),
    /// Nothing: no selector finds the ancestor alone.
    Unfound,
    /// Nothing: the ancestor is the page's body or root, no target.
    Page,
}

/// Why the element takes no press at its middle, its own ancestor on top there.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Why {
    /// It has `pointer-events: none`.
    PointerEventsNone,
    /// Its own `clip`, or its own `clip-path` leaving the point out, hides it there (a
    /// screen-reader-only control).
    OwnClip,
    /// No cause the page or the geometry establishes (nothing of it painted there, as in an SVG
    /// ring's empty middle, among them).
    Unnamed,
}

impl Hit {
    /// A [`probe`]'s `hit`.
    fn read(hit: &Value) -> Hit {
        let text = |key: &str| hit[key].as_str().map(str::to_string);
        let miss = match hit {
            Value::Bool(true) => return Hit::Target,
            Value::String(cover) => Miss::Covered(cover.clone()),
            _ if hit["label"] == true => return Hit::Label,
            _ => {
                if let Some(name) = text("holder") {
                    Miss::Holder(Holder {
                        name,
                        why: match hit["why"].as_str() {
                            Some("none") => Why::PointerEventsNone,
                            Some("clip") => Why::OwnClip,
                            _ => Why::Unnamed,
                        },
                        instead: match text("target") {
                            _ if hit["page"] == true => Instead::Page,
                            Some(target) => Instead::Alone(target),
                            None => Instead::Unfound,
                        },
                        labels: text("labels"),
                    })
                } else if let Some(visibility) = text("hidden") {
                    Miss::Hidden(visibility)
                } else if let Some(by) = text("clipped") {
                    Miss::Clipped(by)
                } else {
                    Miss::Nothing
                }
            }
        };
        Hit::Missed(miss)
    }

    /// What [`CONFIRM`] answered for a press the page took through a label: taken, else covered
    /// by what the browser found there.
    fn confirmed(answer: &Value) -> Hit {
        match answer {
            Value::Bool(true) => Hit::Target,
            Value::String(found) => Hit::Missed(Miss::Covered(found.clone())),
            _ => Hit::Missed(Miss::Covered("what the browser found there".to_string())),
        }
    }
}

/// The browser's own hit at `(x, y)`, in the document's coordinates as the protocol's hit test
/// takes them ([`Place::in_document`]), for a press the page took through a label ([`Hit::Label`],
/// [`Reached::Label`]): the node the browser finds there, inside a closed shadow root too, which
/// the page cannot see into, walked up the flat tree as [`CONFIRM`] walks it, as the press travels.
/// Where a node may be slotted into a closed root the page cannot see (`CONFIRM` answers the node
/// itself), the browser names its slot, `DOM.describeNode`'s `assignedSlot`, and the walk goes on
/// from that slot, or from the node, the browser naming none. Three calls, and two or three for
/// each such node, once for each, paid only for a press through a label. Each moves the walk up
/// the flat tree, so it ends by the document's root, however deep the node, or, on a page that
/// keeps moving its nodes, at the step's wait, as every call of the step does.
fn confirm(chrome: &mut Chrome, css: &str, (x, y): (f64, f64)) -> symphony_chrome::Result<Hit> {
    let found = chrome.call(
        "DOM.getNodeForLocation",
        json!({"x": pixel(x), "y": pixel(y)}),
    )?;
    let hit = object(&chrome.call(
        "DOM.resolveNode",
        json!({"backendNodeId": found["backendNodeId"]}),
    )?);
    let (mut from, mut known) = (hit.clone(), Value::Null);
    loop {
        let known_said = if known.is_null() {
            json!({"value": null})
        } else {
            json!({"objectId": known})
        };
        let answer = chrome.call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": from,
                "functionDeclaration": format!(
                    "function (css, interactive, hit, known) {{ {NAMES}{CONFIRM} }}"
                ),
                "arguments": [{"value": css}, {"value": INTERACTIVE}, {"objectId": hit}, known_said],
            }),
        )?;
        let node = match Walk::read(&answer["result"]) {
            Walk::Answer(answer) => return Ok(Hit::confirmed(&answer)),
            Walk::Ask(node) => node,
        };
        let described = chrome.call("DOM.describeNode", json!({"objectId": node}))?;
        (from, known) = match assigned(&described) {
            Some(slot) => (
                object(&chrome.call("DOM.resolveNode", json!({"backendNodeId": slot}))?),
                Value::Null,
            ),
            None => (node.clone(), node),
        };
    }
}

/// The remote object `DOM.resolveNode` answered, by its id.
fn object(resolved: &Value) -> Value {
    resolved["object"]["objectId"].clone()
}

/// What one walk of [`CONFIRM`] answered.
#[derive(Debug, PartialEq)]
enum Walk {
    /// Its answer, as [`Hit::confirmed`] reads it.
    Answer(Value),
    /// A node, by its remote object's id, whose slot only the browser can name.
    Ask(Value),
}

impl Walk {
    /// `Runtime.callFunctionOn`'s `result`: a node is one to ask about, anything else the answer.
    fn read(result: &Value) -> Walk {
        match &result["objectId"] {
            Value::String(_) if result["subtype"] == "node" => {
                Walk::Ask(result["objectId"].clone())
            }
            _ => Walk::Answer(result["value"].clone()),
        }
    }
}

/// The slot a node is assigned to, by its backend node id, as `DOM.describeNode` described the
/// node: in a closed shadow root too, which the page cannot see.
fn assigned(described: &Value) -> Option<Value> {
    let slot = &described["node"]["assignedSlot"]["backendNodeId"];
    slot.is_u64().then(|| slot.clone())
}

/// Whether the browser's own hit at `(x, y)`, in the document's coordinates as the protocol's hit
/// test takes them, finds a frame ([`FRAMES`]): inside a closed shadow root too, where the page's
/// own hit test stops at the host. Two calls, paid only where no report of a move reached the page
/// and nothing the page sees there is hovered or a frame ([`Arrival::unseen`]). A hit the protocol
/// cannot make (no node there) finds none.
fn frame_at(chrome: &mut Chrome, (x, y): (f64, f64)) -> symphony_chrome::Result<bool> {
    let found = chrome.call(
        "DOM.getNodeForLocation",
        json!({"x": pixel(x), "y": pixel(y)}),
    );
    let described = found.and_then(|found| {
        chrome.call(
            "DOM.describeNode",
            json!({"backendNodeId": found["backendNodeId"]}),
        )
    });
    match described {
        Ok(node) => Ok(is_frame(&node["node"])),
        Err(error) if error.past_deadline() => Err(error),
        Err(_) => Ok(false),
    }
}

/// Whether a node as the protocol describes it is a frame ([`FRAMES`]), by its local name.
fn is_frame(node: &Value) -> bool {
    node["localName"]
        .as_str()
        .is_some_and(|name| FRAMES.split(", ").any(|frame| frame == name))
}

/// A coordinate in the document as the protocol's integers take it: the whole CSS pixel nearest
/// it. A box's middle as the aim takes it lies on one already ([`AIM`]), less what adding the
/// page's scroll back costs in floating point.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a point in the document, in CSS pixels, lies far inside i64"
)]
fn pixel(coordinate: f64) -> i64 {
    coordinate.round() as i64
}

/// The box a click or a hover aims at in an element (its first line box in view that takes the
/// press, [`AIM`]), in the page's coordinates (the layout viewport's, those of its client rects and
/// hit tests), the point it aims at in it (its middle, on a whole pixel of the document), what the
/// screen shows of the page, how far the page is scrolled, in CSS pixels, and what is on top at
/// that point, as a [`probe`] read them in one frame.
#[derive(Clone, Debug, PartialEq)]
struct Place {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    point: (f64, f64),
    view: Viewport,
    scroll: (f64, f64),
    hit: Hit,
}

/// What the screen shows of the page, the visual viewport, in the page's coordinates: its left and
/// top edges and its width and height, in CSS pixels. Where the page is wider than a phone's screen,
/// or zoomed in, it is a part of the layout viewport and moves within it ([`AIM`]); on a desktop,
/// it is the window less its scrollbars.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Viewport {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// Where a click or a hover presses: one point, as the pointer takes it (`screen`, from the visual
/// viewport's corner) and as the page's hit tests take it (`page`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Press {
    screen: (f64, f64),
    page: (f64, f64),
}

impl Place {
    /// What a [`probe`] of `css` answered: the element's place, `None` while nothing matches, or
    /// why `css` is no selector.
    fn read(css: &str, found: &Value) -> Result<Option<Place>, String> {
        if let Some(invalid) = found["invalid"].as_str() {
            return Err(format!("{css}: not a selector: {invalid}"));
        }
        let place = || {
            let point = (found["point"][0].as_f64()?, found["point"][1].as_f64()?);
            Some(Place {
                x: found["x"].as_f64()?,
                y: found["y"].as_f64()?,
                width: found["width"].as_f64()?,
                height: found["height"].as_f64()?,
                point,
                view: Viewport {
                    left: found["view"][0].as_f64()?,
                    top: found["view"][1].as_f64()?,
                    width: found["view"][2].as_f64()?,
                    height: found["view"][3].as_f64()?,
                },
                scroll: (found["scroll"][0].as_f64()?, found["scroll"][1].as_f64()?),
                hit: Hit::read(&found["hit"]),
            })
        };
        Ok(place())
    }

    fn drawn(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    /// Its middle as the aim takes it: the point the probe tested, the box's middle moved to the
    /// nearest whole pixel of the document within the box ([`AIM`]).
    fn middle(&self) -> (f64, f64) {
        self.point
    }

    /// Its middle in the document's coordinates, the page's (the layout viewport's) moved by the
    /// page's scroll, which moves the layout viewport: where `DOM.getNodeForLocation` hit-tests
    /// ([`confirm`]), unlike a pointer event, which takes the visual viewport's
    /// ([`Place::on_screen`]).
    fn in_document(&self) -> (f64, f64) {
        let (x, y) = self.middle();
        (x + self.scroll.0, y + self.scroll.1)
    }

    /// Its middle from the visual viewport's corner: where `Input.dispatchMouseEvent` puts the
    /// pointer for it, in CSS pixels of the view as the screen shows it (Chrome scales them by the
    /// zoom itself). Where the visual viewport is the layout viewport, as on a desktop, it is the
    /// middle itself.
    fn on_screen(&self) -> (f64, f64) {
        let (x, y) = self.middle();
        (x - self.view.left, y - self.view.top)
    }

    /// Where a press on it goes ([`Press`]).
    fn press(&self) -> Press {
        Press {
            screen: self.on_screen(),
            page: self.middle(),
        }
    }

    /// Whether its middle is in the visual viewport, on the screen, where the pointer reaches it;
    /// the far edges are out.
    fn in_view(&self) -> bool {
        let (x, y) = self.middle();
        let view = &self.view;
        (view.left..view.left + view.width).contains(&x)
            && (view.top..view.top + view.height).contains(&y)
    }
}

/// What the latest read of the element a click or a hover aims at found, as its failure says it.
#[derive(Clone, Debug, Default, PartialEq)]
enum Latest {
    /// No read on an animation frame answered.
    #[default]
    Unread,
    Gone,
    Boxless,
    OutOfView,
    /// In view, but not taking a press at its middle, for this reason.
    Missed(Miss),
    /// Drawn in view and on top, but not where the read before found it, or with no read before
    /// to agree.
    Moving,
}

/// What follows a read of the element a click or a hover aims at.
#[derive(Debug, PartialEq)]
enum Next {
    /// Read it again in the next animation frame: it moved, has no box, or is gone, for now.
    Again,
    /// Scroll it to the middle of the view again (its containers and the page), let the page
    /// settle, then read it again: it left the view, or something else is on top at its middle.
    Scroll,
    /// Aim here: two reads in a row found its box in one place, its middle here, in view and the
    /// element on top there.
    At(Press),
}

/// The reads of the element a click or a hover aims at, each in an animation frame, one after
/// another.
#[derive(Debug, Default)]
struct Aim {
    /// The read before, when it found the element drawn and on top with its middle in view.
    held: Option<Place>,
    latest: Latest,
    /// Whether it has been scrolled to the middle of the view (the first scroll is not a re-scroll).
    scrolled: bool,
}

impl Aim {
    /// One more read, `found` (`None` when nothing matched): what follows it.
    fn read(&mut self, found: Option<Place>) -> Next {
        let before = self.held.take();
        let Some(place) = found else {
            self.latest = Latest::Gone;
            return Next::Again;
        };
        if !place.drawn() {
            self.latest = Latest::Boxless;
            return Next::Again;
        }
        if !place.in_view() {
            self.latest = Latest::OutOfView;
            return Next::Scroll;
        }
        if let Hit::Missed(miss) = &place.hit {
            self.latest = Latest::Missed(miss.clone());
            // No scroll shows what its visibility hides; anything else a scroll may clear: what
            // the scroll mounted and moved it under, or out of its scrolling box.
            return match miss {
                Miss::Hidden(_) => Next::Again,
                _ => Next::Scroll,
            };
        }
        self.latest = Latest::Moving;
        let next = match before {
            Some(before) if before == place => Next::At(place.press()),
            _ => Next::Again,
        };
        self.held = Some(place);
        next
    }

    /// Why the aim at `css` was not taken within `wait`, by what the latest read found.
    fn late(&self, css: &str, wait: Wait) -> String {
        let (why, after) = match &self.latest {
            Latest::Unread => (
                "was never read in an animation frame".to_string(),
                String::new(),
            ),
            Latest::Gone => ("left the page".to_string(), String::new()),
            Latest::Boxless => ("is in the page but has no box".to_string(), String::new()),
            Latest::OutOfView => (
                "stayed out of view: its middle was outside the viewport after every scroll"
                    .to_string(),
                String::new(),
            ),
            Latest::Missed(miss) => missed(miss),
            Latest::Moving => (
                "kept moving: no two reads in a row, each in an animation frame, found its box in \
                 one place"
                    .to_string(),
                String::new(),
            ),
        };
        format!("{css} {why} within {}{after}", wait.said())
    }
}

/// Why an element took no press at its middle, as a failure says it: what goes before its wait's
/// length and what after.
fn missed(miss: &Miss) -> (String, String) {
    match miss {
        Miss::Hidden(visibility) => (
            "stayed hidden".to_string(),
            format!(": its visibility is {visibility}, so no press reaches it"),
        ),
        Miss::Clipped(by) => (
            format!("stayed clipped: {by} clips it away at its middle after every scroll"),
            String::new(),
        ),
        Miss::Covered(cover) => (
            format!("stayed covered: {cover} was on top at its middle after every scroll"),
            String::new(),
        ),
        // Never "covered": what is there holds the element, and is named to target instead only
        // when the element takes no pointer, a selector finds it alone, it is not the page's body
        // or root, and a press on it is not a label's for another control.
        Miss::Holder(Holder {
            name,
            why,
            instead,
            labels,
        }) => {
            // A press on what holds an element its own clip hides, or one that misses it for a cause
            // not named, does nothing for it; one that takes no pointer lets the press through to it.
            let substitute = *why == Why::PointerEventsNone;
            let why = match why {
                Why::PointerEventsNone => format!(
                    "takes no pointer at its middle (pointer-events: none): {name}, which holds it, \
                     was on top there after every scroll"
                ),
                // No cause named that the page has not established.
                Why::Unnamed => format!(
                    "takes no pointer at its middle: {name}, which holds it, was on top there after \
                     every scroll"
                ),
                Why::OwnClip => format!(
                    "stayed clipped by its own clip: {name}, which holds it, was on top at its \
                     middle after every scroll"
                ),
            };
            let after = match (labels, instead, substitute) {
                (Some(control), _, _) => {
                    format!("; a press there clicks {control} (by its label), not it")
                }
                (None, _, false) => String::new(),
                (None, Instead::Alone(target), true) => format!("; target {target} instead"),
                (None, Instead::Unfound, true) => {
                    format!("; no selector finds {name} alone, to target instead")
                }
                (None, Instead::Page, true) => {
                    format!("; {name} is the page itself, nothing to target instead")
                }
            };
            (why, after)
        }
        Miss::Nothing => (
            "stayed out of reach: nothing in the page was at its middle after every scroll"
                .to_string(),
            String::new(),
        ),
    }
}

/// How far from the point aimed at, in CSS pixels on either axis, the move's report may put the
/// pointer and still have it there: a mouse event's whole pixels (a pointer event's own point is
/// the aimed one exactly).
const STRAY: f64 = 1.0;

/// What the pointer reached once moved to the point a click or a hover aimed at, as [`ARRIVED`]
/// read it.
#[derive(Clone, Debug, PartialEq)]
struct Arrival {
    /// The element has left the page.
    gone: bool,
    /// Where the move's own report put the pointer, when one came ([`WATCH`]).
    pointer: Option<Pointer>,
    /// What the page's hit test finds at the point aimed at, now.
    hit: Reached,
    /// Whether the element found there is hovered: the pointer is over it.
    hovered: bool,
    /// Whether the element found there is a frame ([`FRAMES`]), whose own document takes the move:
    /// by the page's hit test, or, where that stops at a closed shadow root's host, by the
    /// browser's own ([`frame_at`]).
    framed: bool,
    /// The point aimed at in the document's coordinates, as the browser's own hit test takes it.
    document: Option<(f64, f64)>,
}

/// Where the pointer landed, by the move's own report: its point in the page's coordinates and
/// what it is over.
#[derive(Clone, Debug, PartialEq)]
struct Pointer {
    at: (f64, f64),
    on: Reached,
}

/// What is at a point, for the element aimed at ([`ARRIVED`]).
#[derive(Clone, Debug, PartialEq)]
enum Reached {
    /// The element, something inside it or a label whose activation clicks it, the browser's own
    /// hit agreeing: a press there reaches it.
    Target,
    /// A label whose activation clicks it, as far as the page can see: the browser's own hit is
    /// still to confirm it ([`confirm`]).
    Label,
    /// Another element, by its name.
    Other(String),
    /// No element, as the browser reported it.
    Nothing,
}

impl Reached {
    fn read(value: &Value) -> Reached {
        match value {
            Value::Bool(true) => Reached::Target,
            Value::String(name) => Reached::Other(name.clone()),
            _ if value["label"] == true => Reached::Label,
            _ => Reached::Nothing,
        }
    }

    /// What the browser's own hit made of a press the page took through a label ([`confirm`]).
    fn confirmed(hit: Hit) -> Reached {
        match hit {
            Hit::Target => Reached::Target,
            Hit::Missed(Miss::Covered(found)) => Reached::Other(found),
            Hit::Label | Hit::Missed(_) => Reached::Label,
        }
    }

    /// What it is, as a failure names it.
    fn said(&self) -> &str {
        match self {
            Reached::Target | Reached::Label => "it",
            Reached::Other(name) => name,
            Reached::Nothing => "no element",
        }
    }
}

/// A point as a failure says it, in whole CSS pixels: `(60, 2615)`.
fn point_said((x, y): (f64, f64)) -> String {
    format!("({x:.0}, {y:.0})")
}

impl Arrival {
    /// What [`ARRIVED`] answered.
    fn read(value: &Value) -> Arrival {
        let pointer = &value["pointer"];
        Arrival {
            gone: value["gone"] == true,
            pointer: match (pointer["x"].as_f64(), pointer["y"].as_f64()) {
                (Some(x), Some(y)) => Some(Pointer {
                    at: (x, y),
                    on: Reached::read(&pointer["on"]),
                }),
                _ => None,
            },
            hit: Reached::read(&value["hit"]),
            hovered: value["hovered"] == true,
            framed: value["framed"] == true,
            document: value["document"][0]
                .as_f64()
                .zip(value["document"][1].as_f64()),
        }
    }

    /// Whether nothing the page can tell says where the pointer is: no report of the move, and
    /// what the page's hit test finds there neither hovered nor a frame.
    fn unseen(&self) -> bool {
        self.pointer.is_none() && !self.hovered && !self.framed
    }

    /// Why a click (`clicking`) or a hover fails once the pointer has moved to `aimed`, the point
    /// aimed at in the page's coordinates, after its flag and selector, or `None` to go on.
    ///
    /// A pointer anywhere but where the aim tested fails either: the move's report putting it more
    /// than [`STRAY`] from `aimed`, the point pressed would not be the point tested, so what the
    /// press reaches is unknown to the aim. With no report, a frame found there (in a closed shadow
    /// root too, by the browser's own hit) goes on unconfirmed: its own document took the move,
    /// which the page cannot hear, and one in a process of its own (another site's, or a sandboxed
    /// one of the page's own) leaves its element unhovered. Anything
    /// else found there must be hovered (a page that stops the move before the window hears it),
    /// or the pointer's place is unknown and the step fails. With the pointer there, a hover has
    /// done what it does, and a click fails unless the element is what the hit test finds there
    /// now (or its label, the browser's own hit agreeing): a menu its hover opened over it would
    /// take the press.
    fn refused(&self, aimed: (f64, f64), clicking: bool) -> Option<String> {
        let pressed = if clicking {
            "; nothing was pressed"
        } else {
            ""
        };
        match &self.pointer {
            Some(pointer)
                if (pointer.at.0 - aimed.0).abs() > STRAY
                    || (pointer.at.1 - aimed.1).abs() > STRAY =>
            {
                return Some(format!(
                    ": the pointer landed at {} in the page, on {}, not at its middle {}, where \
                     the aim tested{pressed}",
                    point_said(pointer.at),
                    pointer.on.said(),
                    point_said(aimed)
                ));
            }
            None if self.unseen() => {
                let found = match &self.hit {
                    Reached::Nothing => "the hit test finds no element there".to_string(),
                    hit => format!("{}, found there, is not hovered", hit.said()),
                };
                return Some(format!(
                    ": where the pointer landed, moved to its middle {}, is unknown: no report of \
                     the move reached the page, and {found}{pressed}",
                    point_said(aimed)
                ));
            }
            _ => {}
        }
        if !clicking || self.hit == Reached::Target {
            return None;
        }
        if self.gone {
            return Some(" left the page as the pointer reached it; nothing was pressed".into());
        }
        Some(match &self.hit {
            Reached::Other(name) => format!(
                ": the press would have landed on {name}, on top at its middle once the pointer \
                 reached it; nothing was pressed (for a menu its hover opens, --hover it, then \
                 --click the item meant)"
            ),
            Reached::Label => ": the browser's own hit did not confirm the press through its \
                               label at its middle once the pointer reached it; nothing was \
                               pressed"
                .into(),
            Reached::Target | Reached::Nothing => ": the hit test finds no element at its middle \
                                                   once the pointer reached it; nothing was \
                                                   pressed"
                .into(),
        })
    }
}

/// Wait until an element matches `css` and, when `drawn`, has a box; the page is not scrolled. Or
/// why not within the wait.
fn locate(chrome: &mut Chrome, css: &str, wait: Wait, drawn: bool) -> Result<(), String> {
    let probe = probe(css, Look::Now);
    let mut seen = false;
    let missing = |seen: bool| {
        let why = if seen {
            "is in the page but has no box"
        } else {
            "did not appear"
        };
        format!("{css} {why} within {}", wait.said())
    };
    loop {
        let found = said(chrome.eval(&probe), css, || missing(seen))?;
        match Place::read(css, &found)? {
            Some(place) if !drawn || place.drawn() => return Ok(()),
            place => seen = place.is_some(),
        }
        if wait.over() {
            return Err(missing(seen));
        }
        said(chrome.pump(POLL), css, || missing(seen))?;
    }
}

/// Where a click or a hover (`flag`) aims at the first element `css` matches, once one does and
/// has a box: the middle of the box it aims at (its first line box in view that takes the press,
/// [`AIM`]), scrolled into view, once the page has settled after the scroll as a capture waits and
/// then two reads in a row, each in an animation frame, have found that box in one place with its
/// middle in view and the element taking the press there (or a label whose activation clicks it).
/// What the scroll brings near the view mounts on the frames after it (a `NearViewport`'s
/// placeholder filled, rows above the element) and moves the element meanwhile, out of the view,
/// out of a scrolling container's (clipped) or under a sticky header; one that leaves the view or
/// misses the press any other way but its visibility ([`Miss`]) is scrolled to the middle again,
/// its containers and the page, and the page settled again. Or why not within the wait, the step
/// named, and after a re-scroll the element's latest read.
fn aim(chrome: &mut Chrome, css: &str, wait: Wait, flag: &str) -> Result<Press, String> {
    locate(chrome, css, wait, true).map_err(|why| format!("{flag} {why}"))?;
    let step = format!("{flag} {css}");
    let mut aim = Aim::default();
    let mut next = Next::Scroll;
    loop {
        if next == Next::Scroll {
            let scrolled = said(chrome.eval(&probe(css, Look::Scrolled)), &step, || {
                format!("{flag} {}", aim.late(css, wait))
            })?;
            Place::read(css, &scrolled).map_err(|why| format!("{flag} {why}"))?;
            if aim.scrolled {
                // The wait running out while the page settles after a re-scroll is the element's
                // failure, not the page's: say what its latest read found.
                settled(chrome, wait, &step, &|_, _| {
                    format!("{flag} {}", aim.late(css, wait))
                })?;
            } else {
                settle(chrome, wait, &step)?;
            }
            aim.scrolled = true;
        }
        let found = said(chrome.eval(&probe(css, Look::NextFrame)), &step, || {
            format!("{flag} {}", aim.late(css, wait))
        })?;
        let mut place = Place::read(css, &found).map_err(|why| format!("{flag} {why}"))?;
        // A press the page takes through a label, the browser's own hit confirms or refuses.
        if let Some(place) = place.as_mut().filter(|place| place.hit == Hit::Label) {
            place.hit = said(confirm(chrome, css, place.in_document()), &step, || {
                format!("{flag} {}", aim.late(css, wait))
            })?;
        }
        next = aim.read(place);
        if let Next::At(press) = next {
            return Ok(press);
        }
        if wait.over() {
            return Err(format!("{flag} {}", aim.late(css, wait)));
        }
    }
}

/// Move the mouse to `point` on the screen ([`Press`]).
fn move_to(chrome: &mut Chrome, (x, y): (f64, f64)) -> symphony_chrome::Result<()> {
    chrome.call(
        "Input.dispatchMouseEvent",
        json!({"type": "mouseMoved", "x": x, "y": y, "button": "none", "buttons": 0}),
    )?;
    Ok(())
}

/// Press the left button at `point` on the screen and release it there: a click.
fn press_at(chrome: &mut Chrome, (x, y): (f64, f64)) -> symphony_chrome::Result<()> {
    for (kind, buttons) in [("mousePressed", 1), ("mouseReleased", 0)] {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": x, "y": y, "button": "left", "buttons": buttons, "clickCount": 1}),
        )?;
    }
    Ok(())
}

fn press(chrome: &mut Chrome, key: &Key) -> symphony_chrome::Result<()> {
    let mut down = json!({
        "type": if key.text.is_some() { "keyDown" } else { "rawKeyDown" },
        "key": key.key,
        "code": key.code,
        "windowsVirtualKeyCode": key.key_code,
        "nativeVirtualKeyCode": key.key_code,
        "modifiers": key.modifiers,
    });
    let mut up = down.clone();
    up["type"] = json!("keyUp");
    if let Some(text) = &key.text {
        down["text"] = json!(text);
        down["unmodifiedText"] = json!(text);
    }
    chrome.call("Input.dispatchKeyEvent", down)?;
    chrome.call("Input.dispatchKeyEvent", up)?;
    Ok(())
}

/// What the page says of itself: its document's state, a Symphony page's own navigation marker
/// (`html[data-navigation]`, from the shell), the regions still loading, the regions busy on the
/// screen, the shell's notice and whether it is in front.
///
/// A region is busy while it says so (`aria-busy=true`: a ghost not lifted, a read not in, as a
/// Symphony page marks it), and on the screen while some of its box shows in the window, cut to
/// each box around it whose overflow clips it: one below the window, or scrolled out of its box,
/// draws only once the reader goes to it, and is not waited for.
const STATE: &str = "(() => ({ready: document.readyState, \
navigation: document.documentElement ? (document.documentElement.dataset.navigation ?? null) : null, \
loading: document.querySelectorAll('[data-ready=loading]').length, \
busy: [...document.querySelectorAll('[aria-busy=true]')].filter(region => { \
const r = region.getBoundingClientRect(); \
let top = Math.max(r.top, 0), bottom = Math.min(r.bottom, innerHeight), left = Math.max(r.left, 0), right = Math.min(r.right, innerWidth); \
for (let box = region.parentElement; box && box !== document.body && box !== document.documentElement; box = box.parentElement) { \
const style = getComputedStyle(box); \
if (style.overflowX === 'visible' && style.overflowY === 'visible') continue; \
const c = box.getBoundingClientRect(); \
top = Math.max(top, c.top); bottom = Math.min(bottom, c.bottom); left = Math.max(left, c.left); right = Math.min(right, c.right); } \
return top < bottom && left < right; }).length, \
notice: document.getElementById('module-error')?.textContent ?? null, \
visible: document.visibilityState}))()";

/// Resolves once the page's fonts are in and two frames have been drawn since.
const PAINTED: &str = "document.fonts.ready.then(() => new Promise(resolve => \
requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))))";

const LAYOUT: &str = "({scroll_width: document.documentElement.scrollWidth, \
viewport_width: document.documentElement.clientWidth})";

/// The protocol events that change a page's traffic: a request starting, finishing or failing, and
/// a WebSocket, which Chrome tells of apart, made or closed.
const TRAFFIC: [&str; 5] = [
    "Network.requestWillBeSent",
    "Network.loadingFinished",
    "Network.loadingFailed",
    "Network.webSocketCreated",
    "Network.webSocketClosed",
];

/// Since when the page's traffic has been unchanged: the last request starting, finishing or
/// failing, by when its event was read from the browser (a stream's data is not traffic), but no
/// earlier than `start`, so what a settle follows (a click, a key) has [`QUIET`] to begin one.
fn quiet_since<'a>(events: impl Iterator<Item = (Instant, &'a Value)>, start: Instant) -> Instant {
    events
        .filter(|(_, event)| TRAFFIC.iter().any(|method| event["method"] == *method))
        .map(|(at, _)| at)
        .fold(start, Instant::max)
}

/// Wait until the page has drawn: nothing in [`waiting`], and its traffic unchanged for [`QUIET`]
/// by when the browser's events were read, so a slow look at a busy machine's page does not count
/// against it; then for its fonts and two frames. A page left behind another tab (an Escape the
/// page does not take goes to the browser; a link may open a new tab) draws no frames, so it is
/// brought back in front: first of all, before the page is asked anything, and again whenever it
/// says it is hidden. A page it opened in front of it, of its own site, runs in its process, and a
/// dialog that page shows while in front holds that process, and with it every answer of the
/// page's, until another tab is in front: Chrome then lets the dialog go (an alert as by its OK, a
/// confirm or a prompt as by its Cancel), as it lets a tab behind another alert without holding
/// it. The browser brings the page in front without asking it. Fails when the wait runs out,
/// every call within it included, naming what it still waited for; or at once when the
/// Symphony's shell says the page could not load.
fn settle(chrome: &mut Chrome, wait: Wait, after: &str) -> Result<(), String> {
    settled(chrome, wait, after, &|still, failed| {
        not_drawn(after, wait, still, failed)
    })
}

/// [`settle`], saying a wait that ran out as `late` does from what the page still waited for and
/// the requests that failed meanwhile.
fn settled(
    chrome: &mut Chrome,
    wait: Wait,
    after: &str,
    late: &dyn Fn(&[String], &[Value]) -> String,
) -> Result<(), String> {
    let start = Instant::now();
    let mut still = vec!["the browser to bring the page in front".to_string()];
    let mut failed = Vec::new();
    said(chrome.call("Page.bringToFront", json!({})), after, || {
        late(&still, &failed)
    })?;
    still = vec!["the page to answer".to_string()];
    loop {
        let state = said(chrome.eval(STATE), after, || late(&still, &failed))?;
        if state["navigation"] == "failed" {
            return Err(format!(
                "{after}: the page could not be loaded: {}",
                state["notice"]
                    .as_str()
                    .unwrap_or("the shell says its navigation failed")
                    .trim()
            ));
        }
        if state["visible"] == "hidden" {
            said(chrome.call("Page.bringToFront", json!({})), after, || {
                late(&still, &failed)
            })?;
        }
        let requests = said(chrome.requests(), after, || late(&still, &failed))?;
        failed = requests.iter().filter_map(failure).collect();
        still = waiting(&state, &requests);
        if still.is_empty() && quiet_since(chrome.events_read(), start).elapsed() >= QUIET {
            let painting = ["the page to paint: its fonts, then two frames".to_string()];
            said(chrome.eval(PAINTED), after, || late(&painting, &failed))?;
            return Ok(());
        }
        if still.is_empty() {
            still = vec![format!(
                "its requests to stay quiet for {} ms",
                QUIET.as_millis()
            )];
        }
        if wait.over() {
            return Err(late(&still, &failed));
        }
    }
}

/// A page that was not drawn within its wait: what it still waited for, and the requests that
/// failed meanwhile.
fn not_drawn(after: &str, wait: Wait, still: &[String], failed: &[Value]) -> String {
    let failed = if failed.is_empty() {
        String::new()
    } else {
        let failed: Vec<String> = failed.iter().map(Value::to_string).collect();
        format!("; meanwhile these failed: {}", failed.join(", "))
    };
    format!(
        "{after}: the page was not drawn within {}: still waiting for {}{failed}",
        wait.said(),
        still.join("; ")
    )
}

/// What a page still lacks to count as drawn, in words; empty once it has all of it. The requests
/// in flight of a frame in it are said apart, after the page's own, by the frame's address: a
/// frame whose document is among them as that frame still to load. A worker's are said last, by
/// its script's address.
fn waiting(state: &Value, requests: &[Request]) -> Vec<String> {
    let mut missing = Vec::new();
    if state["visible"] == "hidden" {
        missing.push("the page to be in front (it is hidden)".to_string());
    }
    if let Some(ready) = state["ready"].as_str().filter(|ready| *ready != "complete") {
        missing.push(format!("the document to load (it is {ready})"));
    }
    if state["navigation"] == "loading" {
        missing.push("the page's navigation (html[data-navigation=loading])".to_string());
    }
    if let Some(loading) = state["loading"].as_u64().filter(|loading| *loading > 0) {
        missing.push(format!(
            "{loading} region(s) still loading ([data-ready=loading])"
        ));
    }
    if let Some(busy) = state["busy"].as_u64().filter(|busy| *busy > 0) {
        missing.push(format!(
            "{busy} region(s) busy on the screen ([aria-busy=true])"
        ));
    }
    let pending: Vec<&Request> = requests
        .iter()
        .filter(|request| !request.finished && request.failed.is_none() && !request.streams())
        .collect();
    let own: Vec<&str> = pending
        .iter()
        .filter(|request| request.frame.is_none() && request.worker.is_none())
        .map(|request| request.url.as_str())
        .collect();
    if !own.is_empty() {
        missing.push(in_flight(&own, ""));
    }
    for frame in each(&pending, |request| request.frame.as_deref()) {
        let theirs: Vec<&Request> = pending
            .iter()
            .copied()
            .filter(|request| request.frame.as_deref() == Some(frame))
            .collect();
        if theirs.iter().any(|request| request.kind == "Document") {
            missing.push(format!(
                "the frame {frame} to load (its document is in flight)"
            ));
        } else {
            let urls: Vec<&str> = theirs.iter().map(|request| request.url.as_str()).collect();
            missing.push(in_flight(&urls, &format!(" in the frame {frame}")));
        }
    }
    for worker in each(&pending, |request| request.worker.as_deref()) {
        let urls: Vec<&str> = pending
            .iter()
            .filter(|request| request.worker.as_deref() == Some(worker))
            .map(|request| request.url.as_str())
            .collect();
        missing.push(in_flight(&urls, &format!(" in the worker {worker}")));
    }
    missing
}

/// Each frame or worker `whose` names among `requests`, once, in the order its first was seen.
fn each<'a>(
    requests: &[&'a Request],
    whose: impl Fn(&'a Request) -> Option<&'a str>,
) -> Vec<&'a str> {
    let mut found: Vec<&str> = Vec::new();
    for name in requests.iter().copied().filter_map(whose) {
        if !found.contains(&name) {
            found.push(name);
        }
    }
    found
}

/// Requests in flight, by their addresses (the first three named), `whose` after their count.
fn in_flight(urls: &[&str], whose: &str) -> String {
    let shown = &urls[..urls.len().min(3)];
    let more = urls.len() - shown.len();
    format!(
        "{} request(s) in flight{whose}: {}{}",
        urls.len(),
        shown.join(", "),
        if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        }
    )
}

fn seconds_said(length: Duration) -> String {
    format!("{}s", length.as_secs_f64())
}

/// The page, or with `full_page` all of it, as a PNG at `path`, its folder made, within the wait.
fn capture(chrome: &mut Chrome, path: &Path, full_page: bool, wait: Wait) -> Result<(), String> {
    let capturing = format!("capturing {}", path.display());
    let late = || format!("{capturing}: no picture within {}", wait.said());
    let params = if full_page {
        let metrics = said(
            chrome.call("Page.getLayoutMetrics", json!({})),
            &capturing,
            late,
        )?;
        let size = &metrics["cssContentSize"];
        let (Some(width), Some(height)) = (size["width"].as_f64(), size["height"].as_f64()) else {
            return Err(format!(
                "capturing {}: no content size in {metrics}",
                path.display()
            ));
        };
        json!({"captureBeyondViewport": true, "clip": {"x": 0, "y": 0, "width": width.ceil(), "height": height.ceil(), "scale": 1}})
    } else {
        json!({})
    };
    let picture = said(chrome.screenshot(params), &capturing, late)?;
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    std::fs::write(path, picture).map_err(|error| format!("{}: {error}", path.display()))
}

/// A request that failed, with its error, or was answered 400 or over, with its status; not one
/// the page gave up itself (an abort, or the page going away).
fn failure(request: &Request) -> Option<Value> {
    match &request.failed {
        Some(error) if !request.canceled => Some(json!({"url": request.url, "error": error})),
        Some(_) => None,
        None if request.status >= 400 => {
            Some(json!({"url": request.url, "status": request.status}))
        }
        None => None,
    }
}

/// Which frame each event a report reads came from, learnt as the events go: each followed frame's
/// session (`Target.attachedToTarget`), each execution context's frame on its session
/// (`Runtime.executionContextCreated`), each request's frame (`Network.requestWillBeSent`), each
/// frame's address by its latest navigation, and the page's own frame, the one navigated with no
/// parent.
#[derive(Default)]
struct Origins {
    sessions: HashMap<String, String>,
    contexts: HashMap<(String, i64), String>,
    requests: HashMap<String, String>,
    addresses: HashMap<String, String>,
    page: Option<String>,
}

impl Origins {
    /// What `event` says of the frames.
    fn learn(&mut self, event: &Value) {
        let params = &event["params"];
        let text = |value: &Value| value.as_str().map(str::to_string);
        match event["method"].as_str() {
            Some("Target.attachedToTarget") if params["targetInfo"]["type"] == "iframe" => {
                if let (Some(session), Some(frame)) = (
                    text(&params["sessionId"]),
                    text(&params["targetInfo"]["targetId"]),
                ) {
                    self.sessions.insert(session, frame);
                }
            }
            Some("Runtime.executionContextCreated") => {
                let context = &params["context"];
                if let (Some(id), Some(frame)) =
                    (context["id"].as_i64(), text(&context["auxData"]["frameId"]))
                {
                    self.contexts.insert((session(event), id), frame);
                }
            }
            Some("Network.requestWillBeSent") => {
                if let (Some(id), Some(frame)) =
                    (text(&params["requestId"]), text(&params["frameId"]))
                {
                    self.requests.insert(id, frame);
                }
            }
            Some("Page.frameNavigated") => {
                let frame = &params["frame"];
                if let (Some(id), Some(url)) = (text(&frame["id"]), text(&frame["url"])) {
                    if frame["parentId"].is_null() {
                        self.page = Some(id.clone());
                    }
                    self.addresses.insert(id, url);
                }
            }
            _ => {}
        }
    }

    /// The address of the frame `event` came from, when it is not the page's own and is known: by
    /// the execution context it names (`context`), else by the request it names (`request`), else
    /// by the followed frame's session it came on.
    fn frame(&self, event: &Value, context: Option<i64>, request: Option<&str>) -> Option<&str> {
        let on = session(event);
        let frame = context
            .and_then(|context| self.contexts.get(&(on.clone(), context)))
            .or_else(|| request.and_then(|request| self.requests.get(request)))
            .or_else(|| self.sessions.get(&on))?;
        (self.page.as_ref() != Some(frame))
            .then(|| self.addresses.get(frame).map(String::as_str))
            .flatten()
    }
}

/// The session `event` came on, `""` for the browser's own.
fn session(event: &Value) -> String {
    event["sessionId"].as_str().unwrap_or("").to_string()
}

/// The report: the console's errors and the uncaught exceptions of the page and its frames (a
/// frame's naming its address), the requests that failed (not those the page gave up itself) or
/// were answered 400 or over, the dialogs the shot dismissed (each one's kind, text and the
/// address of the document that opened it), and horizontal overflow.
fn report(events: &[Value], requests: &[Request], layout: &Value) -> Value {
    let mut console: Vec<Value> = Vec::new();
    let mut dialogs = Vec::new();
    let mut origins = Origins::default();
    for event in events {
        origins.learn(event);
        let params = &event["params"];
        let framed = |mut said: Value, context: &Value, request: &Value| {
            if let Some(frame) = origins.frame(event, context.as_i64(), request.as_str()) {
                said["frame"] = json!(frame);
            }
            said
        };
        match event["method"].as_str() {
            Some("Page.javascriptDialogOpening") => dialogs.push(json!({
                "type": params["type"],
                "message": params["message"],
                "url": params["url"],
            })),
            Some("Runtime.consoleAPICalled")
                if matches!(params["type"].as_str(), Some("error" | "assert")) =>
            {
                let text: Vec<String> = params["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|argument| match &argument["value"] {
                        Value::String(text) => text.clone(),
                        Value::Null => argument["description"].as_str().unwrap_or("").to_string(),
                        other => other.to_string(),
                    })
                    .collect();
                console.push(framed(
                    json!({"source": "console", "text": text.join(" ")}),
                    &params["executionContextId"],
                    &Value::Null,
                ));
            }
            Some("Runtime.exceptionThrown") => {
                let details = &params["exceptionDetails"];
                let text = details["exception"]["description"]
                    .as_str()
                    .or_else(|| details["text"].as_str())
                    .unwrap_or("");
                console.push(framed(
                    json!({"source": "exception", "text": text}),
                    &details["executionContextId"],
                    &Value::Null,
                ));
            }
            Some("Log.entryAdded") if params["entry"]["level"] == "error" => {
                let entry = &params["entry"];
                let mut said = json!({"source": entry["source"], "text": entry["text"]});
                if let Some(url) = entry["url"].as_str().filter(|url| !url.is_empty()) {
                    said["url"] = json!(url);
                }
                console.push(framed(said, &Value::Null, &entry["networkRequestId"]));
            }
            _ => {}
        }
    }
    let failed: Vec<Value> = requests.iter().filter_map(failure).collect();
    let scroll = layout["scroll_width"].as_f64().unwrap_or(0.0);
    let viewport = layout["viewport_width"].as_f64().unwrap_or(0.0);
    json!({
        "console_errors": console,
        "failed_requests": failed,
        "dialogs": dialogs,
        "scroll_width": layout["scroll_width"],
        "viewport_width": layout["viewport_width"],
        "horizontal_overflow": scroll > viewport,
    })
}

#[cfg(test)]
mod tests;
