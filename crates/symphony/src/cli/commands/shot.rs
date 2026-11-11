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
  --timeout SECS       how long the page's load and each wait may take before the shot fails
                       naming what it waited for (default 20)
  --size WxH           the viewport in CSS pixels (default 1440x900)
  --phone              a mid phone's screen instead, as the browser tests emulate it: 390x844
                       at 3x, mobile
  --full-page          capture the whole page rather than the viewport
  --scheme light|dark  the colour scheme the page is told the system prefers
  --report             print, as JSON, the console errors and failed requests seen, and the
                       document's scroll width against the viewport's (horizontal overflow)

steps, run in order before the final capture, each as often as needed:
  --wait-for CSS       wait until an element matches CSS
  --click CSS          click the middle of the first element CSS matches
  --type CSS=TEXT      focus that element and type TEXT (CSS ends at the first = outside brackets)
  --key NAME           press a key: Tab, Enter, Escape, Space, Backspace, Delete, ArrowDown,
                       ArrowUp, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown or one
                       character, after Shift+, Control+, Alt+ or Meta+ to hold those
  --hover CSS          move the mouse over the middle of the first element CSS matches
  --eval JS            evaluate JS in the page (awaiting a promise) and print its value as JSON
  --pause MS           wait MS milliseconds
  --shot FILE.png      capture now, mid-sequence (a menu open, then closed, in one run)

A step on an element waits for it; a click, typing, a key or a hover then waits for the page to
settle again, as a capture does. Chrome is SF_CHROME, else the first Chrome found on this machine.
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

fn run(shot: &Shot, binary: &str, out: &mut dyn Write) -> Result<(), String> {
    let program = std::env::current_exe()
        .map_err(|error| format!("this program's own path, to leash Chrome: {error}"))?;
    let mut chrome = Chrome::launch(binary, &Leash::new(program, &[]).quiet())
        .map_err(|error| error.to_string())?;
    prepare(&mut chrome, shot)?;
    chrome
        .navigate(&shot.url, shot.timeout)
        .map_err(|error| format!("{} did not load: {error}", shot.url))?;
    settle(&mut chrome, shot.timeout, &shot.url)?;
    for step in &shot.steps {
        step.run(&mut chrome, shot, out)?;
    }
    settle(&mut chrome, shot.timeout, "the final capture")?;
    capture(&mut chrome, &shot.out, shot.full_page)?;
    if shot.report {
        let requests = chrome.requests().map_err(|error| error.to_string())?;
        let layout = chrome.eval(LAYOUT).map_err(|error| error.to_string())?;
        let said = report(chrome.events(), &requests, &layout);
        writeln!(out, "{said}").map_err(|error| format!("printing the report: {error}"))?;
    }
    Ok(())
}

/// The viewport, the colour scheme and, for a report, the console's domains, before the page loads.
fn prepare(chrome: &mut Chrome, shot: &Shot) -> Result<(), String> {
    match shot.view {
        View::Phone => chrome.phone_screen(),
        View::Size(width, height) => chrome
            .call(
                "Emulation.setDeviceMetricsOverride",
                json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
            )
            .map(drop),
    }
    .map_err(|error| error.to_string())?;
    if let Some(scheme) = &shot.scheme {
        let value = match scheme {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        };
        chrome
            .call(
                "Emulation.setEmulatedMedia",
                json!({"features": [{"name": "prefers-color-scheme", "value": value}]}),
            )
            .map_err(|error| error.to_string())?;
    }
    if shot.report {
        for domain in ["Runtime.enable", "Log.enable"] {
            chrome
                .call(domain, json!({}))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

impl Step {
    fn run(&self, chrome: &mut Chrome, shot: &Shot, out: &mut dyn Write) -> Result<(), String> {
        let timeout = shot.timeout;
        match self {
            Step::WaitFor(css) => {
                locate(chrome, css, timeout, false).map_err(|why| format!("--wait-for {why}"))?;
            }
            Step::Click(css) | Step::Hover(css) => {
                let clicking = matches!(self, Step::Click(_));
                let flag = if clicking { "--click" } else { "--hover" };
                let point =
                    locate(chrome, css, timeout, true).map_err(|why| format!("{flag} {why}"))?;
                pointer(chrome, point, clicking)
                    .map_err(|error| format!("{flag} {css}: {error}"))?;
                settle(chrome, timeout, &format!("{flag} {css}"))?;
            }
            Step::Type(css, text) => {
                locate(chrome, css, timeout, false).map_err(|why| format!("--type {why}"))?;
                let focused = chrome
                    .eval(&format!(
                        "(() => {{ const element = document.querySelector({}); element.scrollIntoView({{block: 'center', inline: 'center'}}); element.focus(); return document.activeElement === element; }})()",
                        json!(css)
                    ))
                    .map_err(|error| format!("--type {css}: {error}"))?;
                if focused != json!(true) {
                    return Err(format!("--type {css}: the element cannot take focus"));
                }
                chrome
                    .call("Input.insertText", json!({"text": text}))
                    .map_err(|error| format!("--type {css}: {error}"))?;
                settle(chrome, timeout, &format!("--type {css}"))?;
            }
            Step::Key(key) => {
                press(chrome, key).map_err(|error| format!("--key {}: {error}", key.key))?;
                settle(chrome, timeout, &format!("--key {}", key.key))?;
            }
            Step::Eval(script) => {
                let value = chrome
                    .eval(script)
                    .map_err(|error| format!("--eval: {error}"))?;
                writeln!(out, "{value}").map_err(|error| format!("--eval: printing: {error}"))?;
            }
            Step::Pause(length) => {
                let until = Instant::now() + *length;
                while let Some(left) = until
                    .checked_duration_since(Instant::now())
                    .filter(|left| !left.is_zero())
                {
                    chrome
                        .pump(left.min(PAUSE_SLICE))
                        .map_err(|error| format!("--pause: {error}"))?;
                }
            }
            Step::Shot(path) => {
                settle(chrome, timeout, &format!("--shot {}", path.display()))?;
                capture(chrome, path, shot.full_page)?;
            }
        }
        Ok(())
    }
}

/// The middle of the first element `css` matches, once one does (and, when `drawn`, once it has a
/// box), scrolled into view; or why not within `timeout`.
fn locate(
    chrome: &mut Chrome,
    css: &str,
    timeout: Duration,
    drawn: bool,
) -> Result<(f64, f64), String> {
    let deadline = Instant::now() + timeout;
    let probe = format!(
        "(() => {{ let element; try {{ element = document.querySelector({}); }} catch (error) {{ return {{invalid: String(error.message)}}; }} \
         if (!element) return null; element.scrollIntoView({{block: 'center', inline: 'center'}}); \
         const box = element.getBoundingClientRect(); return {{x: box.x + box.width / 2, y: box.y + box.height / 2, drawn: box.width > 0 && box.height > 0}}; }})()",
        json!(css)
    );
    loop {
        let found = chrome
            .eval(&probe)
            .map_err(|error| format!("{css}: {error}"))?;
        if let Some(invalid) = found["invalid"].as_str() {
            return Err(format!("{css}: not a selector: {invalid}"));
        }
        let point = found["x"].as_f64().zip(found["y"].as_f64());
        if let Some(point) = point.filter(|_| !drawn || found["drawn"] == json!(true)) {
            return Ok(point);
        }
        if Instant::now() >= deadline {
            let why = if point.is_some() {
                "is in the page but has no box"
            } else {
                "did not appear"
            };
            return Err(format!("{css} {why} within {}", seconds_said(timeout)));
        }
        chrome
            .pump(POLL)
            .map_err(|error| format!("{css}: {error}"))?;
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
/// (`html[data-navigation]`, from the shell), the regions still loading and the shell's notice.
const STATE: &str = "(() => ({ready: document.readyState, \
navigation: document.documentElement ? (document.documentElement.dataset.navigation ?? null) : null, \
loading: document.querySelectorAll('[data-ready=loading]').length, \
notice: document.getElementById('module-error')?.textContent ?? null}))()";

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
/// against it; then for its fonts and two frames. Fails after `timeout` naming what it still
/// waited for, or at once when the Symphony's shell says the page could not load.
fn settle(chrome: &mut Chrome, timeout: Duration, after: &str) -> Result<(), String> {
    let start = Instant::now();
    let deadline = start + timeout;
    loop {
        let state = chrome
            .eval(STATE)
            .map_err(|error| format!("{after}: {error}"))?;
        if state["navigation"] == "failed" {
            return Err(format!(
                "{after}: the page could not be loaded: {}",
                state["notice"]
                    .as_str()
                    .unwrap_or("the shell says its navigation failed")
                    .trim()
            ));
        }
        let requests = chrome
            .requests()
            .map_err(|error| format!("{after}: {error}"))?;
        let missing = waiting(&state, &requests);
        if missing.is_empty() && quiet_since(chrome.events_read(), start).elapsed() >= QUIET {
            chrome
                .eval(PAINTED)
                .map_err(|error| format!("{after}: {error}"))?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            let missing = if missing.is_empty() {
                vec![format!(
                    "its requests to stay quiet for {} ms",
                    QUIET.as_millis()
                )]
            } else {
                missing
            };
            let failed: Vec<String> = requests
                .iter()
                .filter_map(failure)
                .map(|failure| failure.to_string())
                .collect();
            let failed = if failed.is_empty() {
                String::new()
            } else {
                format!("; meanwhile these failed: {}", failed.join(", "))
            };
            return Err(format!(
                "{after}: the page was not drawn within {}: still waiting for {}{failed}",
                seconds_said(timeout),
                missing.join("; ")
            ));
        }
    }
}

/// What a page still lacks to count as drawn, in words; empty once it has all of it.
fn waiting(state: &Value, requests: &[Request]) -> Vec<String> {
    let mut missing = Vec::new();
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

/// The page, or with `full_page` all of it, as a PNG at `path`, its folder made.
fn capture(chrome: &mut Chrome, path: &Path, full_page: bool) -> Result<(), String> {
    let params = if full_page {
        let metrics = chrome
            .call("Page.getLayoutMetrics", json!({}))
            .map_err(|error| format!("capturing {}: {error}", path.display()))?;
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
    let picture = chrome
        .screenshot(params)
        .map_err(|error| format!("capturing {}: {error}", path.display()))?;
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
