//! `symphony shot URL --out FILE.png`: a page as an agent sees it. A private headless Chrome
//! (crates/chrome-devtools: a profile and process of its own, leashed to this one and killed by its
//! recorded group on every exit) opens URL, waits until the page has drawn, runs the steps given in
//! their order (clicks, typing, keys, hovering, scripts, pauses, captures) and saves a PNG. It reads
//! the page only through the browser: it never starts, stops or reconfigures a Symphony server.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrome_devtools::{Chrome, Leash, Request};
use serde_json::{Value, json};
use symphony_args::{Flag, parse};

const USAGE: &str = "usage: symphony shot [-h] URL --out FILE.png [--wait-for CSS] [--timeout SECS] \
[--size WxH | --phone] [--full-page] [--scheme light|dark] [--report] [--click CSS] [--type CSS=TEXT] \
[--key NAME] [--hover CSS] [--eval JS] [--pause MS] [--shot FILE.png] …";
const HELP: &str =
    "A page as an agent sees it: a private headless Chrome opens URL, waits until the page
has drawn (its document loaded, a Symphony page's own navigation ready, no region still loading,
and no request but a feed stream in flight for 300 ms), runs the steps in the order given, and saves
a PNG. It only reads the page: no server is started, stopped or changed.

positional arguments:
  URL                  the page, an absolute URL (http://127.0.0.1:8765/symphony/tasks)

options:
  -h, --help           show this help message and exit
  --out FILE.png       where the final picture goes (its folder is made)
  --timeout SECS       how long the page's load, each step and the final capture may each take,
                       every call to the browser within them included, before the shot fails
                       naming what it waited for (default 20; a pause adds its own length)
  --size WxH           the viewport in CSS pixels (default 1440x900)
  --phone              a mid phone's screen instead, as the browser tests emulate it: 390x844
                       at 3x, mobile
  --full-page          capture the whole page rather than the viewport
  --scheme light|dark  the colour scheme the page is told the system prefers
  --report             print, as JSON, the console errors and failed requests seen, and the
                       document's scroll width against the viewport's (horizontal overflow)

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
line boxes: the first whose middle is in view and takes the press, else the first in view that no
box around it clips there, so a link wrapped over two lines is aimed at its first line, and one
taller than its scrolling box at a line the box shows. It aims only once the page has settled
after the scroll, as a capture waits, and two reads in a row, each in an animation frame, find
that box in one place with its middle in view and taking the press: on top there is the element,
something inside it, or the label of the control it is (a checkbox hidden in its label or under
the label's styled track), pressed anywhere in the label but on a link, an object or another
control inside it (in a component's shadow root too), as the label's activation clicks its
control. Content the scroll brings near mounts and moves it first, and can push it out of its
scrolling container's view or under a sticky header: should it leave the view, be clipped or be
covered, it is scrolled to the middle again (its containers and the page, clear of a header
shorter than half the view) and the page settled again. An element that never holds still, or
stays covered, fails the step (naming what covers it), and nothing is pressed. So does one that
stays clipped (naming the box that clips it away), stays hidden (its visibility), or takes no
pointer at its middle, its own ancestor on top there (pointer-events: none, its own clip, or
nothing of it painted there, as in an SVG ring): a press would reach the ancestor, which may not
be what was meant, so the failure names it, with a selector that finds it alone to target
instead (or says none does), unless the element's own clip hides it or a press there clicks
another control through a label. Typing focuses its element by script and types with no pointer,
so what covers the element takes none of it. A click, typing, a key or a hover then waits for the page to settle
again, as a capture does, bringing the page back in front if the step left it hidden (an Escape
the page does not take).
Chrome is SF_CHROME, else the first Chrome found on this machine.
Exit 0 with every picture saved, 1 when the browser, the page or a step failed, 2 for a command
line it cannot run.";

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
    report: bool,
    steps: Vec<Step>,
}

const FLAGS: [Flag; 15] = [
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
        name: "--report",
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
    let Some(binary) = chrome_devtools::find() else {
        let _ = writeln!(
            err,
            "symphony shot: no Chrome on this machine: name its executable with SF_CHROME"
        );
        return 1;
    };
    if !Path::new(&binary).is_file() {
        let _ = writeln!(
            err,
            "symphony shot: Chrome at {binary} is not a file: name its executable with SF_CHROME"
        );
        return 1;
    }
    match run(&shot, &binary, out) {
        Ok(()) => 0,
        Err(problem) => {
            let _ = writeln!(err, "symphony shot: {problem}");
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
    let mut report = false;
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
            "--report" => report = true,
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
        report,
        steps,
    }))
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
    result: chrome_devtools::Result<T>,
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

/// The viewport, the colour scheme and, for a report, the console's domains, before the page loads.
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
    if shot.report {
        for domain in ["Runtime.enable", "Log.enable"] {
            said(chrome.call(domain, json!({})), domain, late)?;
        }
    }
    Ok(())
}

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
                let point = aim(chrome, css, wait, flag)?;
                let step = format!("{flag} {css}");
                said(pointer(chrome, point, clicking), &step, || {
                    format!("{step}: the browser did not answer within {}", wait.said())
                })?;
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
/// land, `hit`.
///
/// What is under a point is `reach`ed as Chrome's pointer finds it: the document's element there
/// (`at`, a shadow host for what its shadow root draws, as a selector can name), then through every
/// open shadow root to the element really there (`deep`), and the first interactive element
/// ([`INTERACTIVE`]) from that one up, through shadow hosts, as a label's activation looks for one:
/// when it is a label, `via` is the control it clicks.
///
/// What clips the element is `clips`: its own `clip`, then each box around it that can clip it
/// (the boxes its containing blocks are in, so an absolute or a fixed one escapes the boxes it
/// does not lie in) by its `clip`, its overflow other than `visible` on an axis (a scrolling box
/// among them) or its paint containment, to the box's padding edge. The page and its body are the
/// view's.
///
/// `box` is the first of the element's line boxes (its client rects with an area) whose middle is
/// in the viewport and takes the press there, else the first in view clipped by nothing there,
/// else the first in view, else its first, else its bounding box: a link wrapped over two lines
/// is aimed at its first line, not at the middle of its bounding box, which may hold none of it,
/// and one taller than the view, or than the scrolling box it is in, at a line that box shows.
///
/// `hit`, at the box's middle, is `true` when the element takes the press: what is there is the
/// element or inside it, or it is a label whose control the element is (`label.control`, by `for`
/// or by holding it) and nothing interactive lies between, as the label's activation then clicks
/// it (a checkbox hidden in its label or under the label's styled track). Else it is why not, the
/// first that holds: `{hidden}` for an element whose visibility is not `visible`; `{clipped}` for
/// the box around it that clips it away there; a cover's name for what is there and does not hold
/// the element (the element in a shadow root there named in its host's: `button#inner in
/// x-pressable#host's shadow root`); and `{holder, why, target, labels}` for its own ancestor there, which a press
/// there reaches instead, `why` being `clip` for the element's own clip, `none` for its
/// `pointer-events: none`, else `unpainted` (nothing of it painted there, as in an SVG ring),
/// `target` a selector that finds the ancestor alone, and `labels` the control a press there
/// clicks through a label. `null` is nothing there (a point outside the page).
///
/// An element is named as a selector names it, its tag, id and first three classes after its
/// parent's when it has neither id nor class (`main > div`), which need not find it alone; `alone`
/// gives one that does, that name or a path of them by `:nth-of-type` from it up, its own place
/// among its kin always after its parent's name (`nav.tools > button:nth-of-type(2)`), or `null`
/// for an element in a shadow root, or none found.
const AIM: &str = r#"
const name = e => e.localName + (e.id ? '#' + CSS.escape(e.id) : '') + [...e.classList].slice(0, 3).map(c => '.' + CSS.escape(c)).join('');
const named = e => (!e.id && !e.classList.length && e.parentElement ? name(e.parentElement) + ' > ' : '') + name(e);
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
const up = e => e.parentElement || (e.parentNode instanceof ShadowRoot ? e.parentNode.host : null);
const flat = e => e.assignedSlot || up(e);
const reach = (x, y) => {
  const at = document.elementFromPoint(x, y);
  let deep = at;
  for (let depth = 0; deep && deep.shadowRoot && depth < 32; depth++) {
    const inner = deep.shadowRoot.elementFromPoint(x, y);
    if (!inner || inner === deep) break;
    deep = inner;
  }
  let first = deep;
  while (first && !first.matches(INTERACTIVE)) first = up(first);
  return {at, deep, via: first && first.localName === 'label' ? first.control : null};
};
const takes = p => !!p.at && (element.contains(p.at) || p.via === element);
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
const middle = r => [r.x + r.width / 2, r.y + r.height / 2];
const seen = ([x, y]) => x >= 0 && x < innerWidth && y >= 0 && y < innerHeight;
const rects = [...element.getClientRects()].filter(r => r.width > 0 && r.height > 0);
const viewed = rects.filter(r => seen(middle(r)));
const box = viewed.find(r => takes(reach(...middle(r)))) ?? viewed.find(r => !clips.some(c => away(c, ...middle(r)))) ?? viewed[0] ?? rects[0] ?? element.getBoundingClientRect();
const [x, y] = middle(box);
const p = reach(x, y);
const clipper = clips.find(c => c.by && away(c, x, y));
let held = false;
for (let e = element; e && !held; e = flat(e)) held = e === p.deep;
const hit = !p.at ? null
  : takes(p) ? true
  : style.visibility !== 'visible' ? {hidden: style.visibility}
  : clipper ? {clipped: named(clipper.by)}
  : !held ? (p.deep === p.at ? named(p.at) : named(p.deep) + ' in ' + named(p.at) + "'s shadow root")
  : {holder: named(p.at), why: clips.some(c => !c.by && away(c, x, y)) ? 'clip' : style.pointerEvents === 'none' ? 'none' : 'unpainted', target: alone(p.at), labels: p.via && (alone(p.via) ?? named(p.via))};
"#;

/// What, pressed inside a label, takes the press itself rather than activating the label (HTML's
/// interactive content, and an object, whose own document takes a press as an iframe's does): a
/// press on a link or another control inside a label, a component's shadow root among the places
/// it lies, never reaches the label's control.
const INTERACTIVE: &str = "a[href], audio[controls], button, details, embed, iframe, img[usemap], input, label, object, select, textarea, video[controls]";

/// A script reading the box a click or a hover aims at in the first element `css` matches, the
/// viewport's size and what is on top at the box's middle ([`AIM`]), as `look` says: `null` while
/// nothing matches, `{invalid}` for what is no selector.
fn probe(css: &str, look: Look) -> String {
    let read = format!(
        "(() => {{ let element; try {{ element = document.querySelector({}); }} catch (error) {{ return {{invalid: String(error.message)}}; }} \
         if (!element) return null; {} \
         const INTERACTIVE = {}; {AIM} \
         return {{x: box.x, y: box.y, width: box.width, height: box.height, view: [innerWidth, innerHeight], hit}}; }})()",
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
    /// clicks it ([`AIM`]).
    Target,
    /// Why not.
    Missed(Miss),
}

/// Why an element does not take a press at the middle of its box, the first that holds ([`AIM`]).
#[derive(Clone, Debug, PartialEq)]
enum Miss {
    /// Its visibility is not `visible` (it is `hidden` or `collapse`), so no press reaches it.
    Hidden(String),
    /// A box around it, by its name, clips it away there (its overflow, its paint containment or
    /// its `clip`): it is scrolled out of its scrolling box's view, or folded away.
    Clipped(String),
    /// Another element, by its name, which does not hold it.
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
    /// A selector that finds it alone, when there is one.
    target: Option<String>,
    /// The control a press there clicks through a label (the holder's or one around it), by a
    /// selector that finds it alone, else its name: no substitute for the element.
    labels: Option<String>,
}

/// Why the element takes no press at its middle, its own ancestor on top there.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Why {
    /// It has `pointer-events: none`.
    PointerEventsNone,
    /// Its own `clip` hides it there (a screen-reader-only control).
    OwnClip,
    /// Nothing of it is painted there (an SVG ring's empty middle).
    Unpainted,
}

impl Hit {
    /// A [`probe`]'s `hit`.
    fn read(hit: &Value) -> Hit {
        let text = |key: &str| hit[key].as_str().map(str::to_string);
        let miss = match hit {
            Value::Bool(true) => return Hit::Target,
            Value::String(cover) => Miss::Covered(cover.clone()),
            _ => {
                if let Some(name) = text("holder") {
                    Miss::Holder(Holder {
                        name,
                        why: match hit["why"].as_str() {
                            Some("none") => Why::PointerEventsNone,
                            Some("clip") => Why::OwnClip,
                            _ => Why::Unpainted,
                        },
                        target: text("target"),
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
}

/// The box a click or a hover aims at in an element (its first line box in view that takes the
/// press, [`AIM`]) and
/// the viewport's size, in CSS pixels, and what is on top at the box's middle, as a [`probe`] read
/// them.
#[derive(Clone, Debug, PartialEq)]
struct Place {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    view: (f64, f64),
    hit: Hit,
}

impl Place {
    /// What a [`probe`] of `css` answered: the element's place, `None` while nothing matches, or
    /// why `css` is no selector.
    fn read(css: &str, found: &Value) -> Result<Option<Place>, String> {
        if let Some(invalid) = found["invalid"].as_str() {
            return Err(format!("{css}: not a selector: {invalid}"));
        }
        let place = || {
            Some(Place {
                x: found["x"].as_f64()?,
                y: found["y"].as_f64()?,
                width: found["width"].as_f64()?,
                height: found["height"].as_f64()?,
                view: (found["view"][0].as_f64()?, found["view"][1].as_f64()?),
                hit: Hit::read(&found["hit"]),
            })
        };
        Ok(place())
    }

    fn drawn(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    fn middle(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Whether its middle is in the viewport, where the pointer reaches it.
    fn in_view(&self) -> bool {
        let (x, y) = self.middle();
        (0.0..self.view.0).contains(&x) && (0.0..self.view.1).contains(&y)
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
    At((f64, f64)),
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
            Some(before) if before == place => Next::At(place.middle()),
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
        // when a selector finds it alone and a press on it is not a label's for another control.
        Miss::Holder(Holder {
            name,
            why,
            target,
            labels,
        }) => {
            // A press on what holds an element its own clip hides is no substitute for it.
            let substitute = *why != Why::OwnClip;
            let why = match why {
                Why::PointerEventsNone => format!(
                    "takes no pointer at its middle (pointer-events: none): {name}, which holds it, \
                     was on top there after every scroll"
                ),
                Why::Unpainted => format!(
                    "takes no pointer at its middle: {name}, which holds it, was on top there after \
                     every scroll"
                ),
                Why::OwnClip => format!(
                    "stayed clipped by its own clip: {name}, which holds it, was on top at its \
                     middle after every scroll"
                ),
            };
            let after = match (labels, target, substitute) {
                (Some(control), _, _) => {
                    format!("; a press there clicks {control} (by its label), not it")
                }
                (None, _, false) => String::new(),
                (None, Some(target), true) => format!("; target {target} instead"),
                (None, None, true) => {
                    format!("; no selector finds {name} alone, to target instead")
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
fn aim(chrome: &mut Chrome, css: &str, wait: Wait, flag: &str) -> Result<(f64, f64), String> {
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
        next = aim.read(Place::read(css, &found).map_err(|why| format!("{flag} {why}"))?);
        if let Next::At(point) = next {
            return Ok(point);
        }
        if wait.over() {
            return Err(format!("{flag} {}", aim.late(css, wait)));
        }
    }
}

/// Move the mouse to `point`, and click there when `click`.
fn pointer(chrome: &mut Chrome, (x, y): (f64, f64), click: bool) -> chrome_devtools::Result<()> {
    chrome.call(
        "Input.dispatchMouseEvent",
        json!({"type": "mouseMoved", "x": x, "y": y, "button": "none", "buttons": 0}),
    )?;
    if click {
        for (kind, buttons) in [("mousePressed", 1), ("mouseReleased", 0)] {
            chrome.call(
                "Input.dispatchMouseEvent",
                json!({"type": kind, "x": x, "y": y, "button": "left", "buttons": buttons, "clickCount": 1}),
            )?;
        }
    }
    Ok(())
}

fn press(chrome: &mut Chrome, key: &Key) -> chrome_devtools::Result<()> {
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
/// (`html[data-navigation]`, from the shell), the regions still loading, the shell's notice and
/// whether it is in front.
const STATE: &str = "(() => ({ready: document.readyState, \
navigation: document.documentElement ? (document.documentElement.dataset.navigation ?? null) : null, \
loading: document.querySelectorAll('[data-ready=loading]').length, \
notice: document.getElementById('module-error')?.textContent ?? null, \
visible: document.visibilityState}))()";

/// Resolves once the page's fonts are in and two frames have been drawn since.
const PAINTED: &str = "document.fonts.ready.then(() => new Promise(resolve => \
requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))))";

const LAYOUT: &str = "({scroll_width: document.documentElement.scrollWidth, \
viewport_width: document.documentElement.clientWidth})";

/// The protocol events that change a page's traffic: a request starting, finishing or failing.
const TRAFFIC: [&str; 3] = [
    "Network.requestWillBeSent",
    "Network.loadingFinished",
    "Network.loadingFailed",
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
/// brought back in front. Fails when the wait runs out, every call within it included, naming what
/// it still waited for; or at once when the Symphony's shell says the page could not load.
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
    let mut still = vec!["the page to answer".to_string()];
    let mut failed = Vec::new();
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

/// What a page still lacks to count as drawn, in words; empty once it has all of it.
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
    let pending: Vec<&str> = requests
        .iter()
        .filter(|request| {
            !request.finished && request.failed.is_none() && request.kind != "EventSource"
        })
        .map(|request| request.url.as_str())
        .collect();
    if !pending.is_empty() {
        let shown: Vec<&str> = pending.iter().take(3).copied().collect();
        let more = pending.len() - shown.len();
        missing.push(format!(
            "{} request(s) in flight: {}{}",
            pending.len(),
            shown.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ));
    }
    missing
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

/// The report: the console's errors and the page's uncaught exceptions, the requests that failed
/// (not those the page gave up itself) or were answered 400 or over, and horizontal overflow.
fn report(events: &[Value], requests: &[Request], layout: &Value) -> Value {
    let mut console = Vec::new();
    for event in events {
        let params = &event["params"];
        match event["method"].as_str() {
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
                console.push(json!({"source": "console", "text": text.join(" ")}));
            }
            Some("Runtime.exceptionThrown") => {
                let details = &params["exceptionDetails"];
                let text = details["exception"]["description"]
                    .as_str()
                    .or_else(|| details["text"].as_str())
                    .unwrap_or("");
                console.push(json!({"source": "exception", "text": text}));
            }
            Some("Log.entryAdded") if params["entry"]["level"] == "error" => {
                let entry = &params["entry"];
                let mut said = json!({"source": entry["source"], "text": entry["text"]});
                if let Some(url) = entry["url"].as_str().filter(|url| !url.is_empty()) {
                    said["url"] = json!(url);
                }
                console.push(said);
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
        "scroll_width": layout["scroll_width"],
        "viewport_width": layout["viewport_width"],
        "horizontal_overflow": scroll > viewport,
    })
}

#[cfg(test)]
mod tests;
