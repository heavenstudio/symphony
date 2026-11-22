//! A plugin answering from a process of its own: its binary's `main` is [`serve`]`(&PLUGIN)`, and
//! the host that starts it speaks the protocol on its stdin and stdout (docs/engineering/PLUGIN_PROTOCOL.md).
//!
//! Each line read is an [`Envelope`]; a request is answered on one of [`WORKERS`] threads, so a slow
//! call holds one worker and the others go on answering, and answers may come back in another order
//! than their requests. A request that does not read, a call this plugin has no object for, or a
//! panic in the call is answered `{"refused": …}` naming why, by the request's id; so is a call
//! whose answer would be a line longer than [`LINE_MAX`]. A line that is no envelope (not JSON,
//! not UTF-8) names no call and is skipped. A cancel is read and changes nothing: a call runs to
//! its end and its answer is written, which the host drops. The process ends when its stdin does,
//! once every call read is answered, and under [`serve`] at most [`ENDING`] after, whatever its
//! calls are doing; a line read longer than [`LINE_MAX`] ends it too, said on stderr.

use std::collections::VecDeque;
use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::value::RawValue;

use crate::protocol::{
    Answered, Declaration, Envelope, KindAnswers, LINE_MAX, Lined, Member, PageData, ProblemData,
    Raw, Request, read_line,
};
use crate::{Docs, Page, Plugin};

/// The calls a process answers at once.
pub const WORKERS: usize = 4;

/// How long a process served by [`serve`] goes on once its stdin has ended, for the calls still
/// running to answer: then it exits with code 1, whatever they are doing, so a call that never
/// returns does not keep it past its host.
pub const ENDING: Duration = Duration::from_secs(1);

/// The renders whose pages a process keeps, the newest: a render's components share their `once`
/// values while its page is among them ([`PageData::render`]).
const KEPT_RENDERS: usize = 32;

/// The plugin's process: with no argument it answers the protocol on stdin and stdout until stdin
/// ends ([`ENDING`]); `command NAME ARGS…` runs the plugin's command `NAME` with `ARGS`, writing to
/// this process's own stdout and stderr, and exits with its code.
pub fn serve(plugin: &dyn Plugin) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out, err) = (&mut io::stdout(), &mut io::stderr());
    let code = if args.is_empty() {
        let name = plugin.name().to_string();
        let exit = Box::new(move || {
            let _ = writeln!(
                io::stderr(),
                "{name}: its input ended and a call still runs {ENDING:?} after: the process exits"
            );
            std::process::exit(1)
        });
        main(
            plugin,
            &args,
            Ending::new(io::stdin().lock(), ENDING, exit),
            out,
            err,
        )
    } else {
        main(plugin, &args, io::stdin().lock(), out, err)
    };
    ExitCode::from(code)
}

/// An input that, once it ends or fails, runs `then` `after` later on a thread of its own: what
/// ends a process whose calls outlive its input, unless it has exited by then.
struct Ending<R> {
    input: R,
    after: Duration,
    then: Option<Box<dyn FnOnce() + Send>>,
}

impl<R: BufRead> Ending<R> {
    fn new(input: R, after: Duration, then: Box<dyn FnOnce() + Send>) -> Self {
        Self {
            input,
            after,
            then: Some(then),
        }
    }
}

impl<R: BufRead> Read for Ending<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let taken = available.len().min(buf.len());
        buf[..taken].copy_from_slice(&available[..taken]);
        self.consume(taken);
        Ok(taken)
    }
}

impl<R: BufRead> BufRead for Ending<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        let Self { input, after, then } = self;
        let read = input.fill_buf();
        let ended = match &read {
            Ok(available) => available.is_empty(),
            Err(e) => e.kind() != io::ErrorKind::Interrupted,
        };
        if ended && let Some(then) = then.take() {
            let after = *after;
            // With no thread to wait on it, the process ends when its calls do.
            let _ = std::thread::Builder::new()
                .name("ending".into())
                .spawn(move || {
                    #[expect(clippy::disallowed_methods, reason = "no-timed-waits baseline: a thread sleeps out a deadline, where a deadline on the wait itself is the bound")]
                    std::thread::sleep(after);
                    then();
                });
        }
        read
    }

    fn consume(&mut self, amount: usize) {
        self.input.consume(amount);
    }
}

/// [`serve`] with `args`, its input, output and errors given: its exit code.
fn main(
    plugin: &dyn Plugin,
    args: &[String],
    input: impl BufRead,
    out: &mut (impl Write + Send),
    err: &mut dyn Write,
) -> u8 {
    match args.split_first() {
        None => match serve_on(plugin, input, out) {
            Ok(()) => 0,
            Err(why) => {
                let _ = writeln!(err, "{}: the protocol's input failed: {why}", plugin.name());
                1
            }
        },
        Some((first, rest)) if first == "command" => {
            let Some((name, args)) = rest.split_first() else {
                let _ = writeln!(err, "usage: command NAME [ARGS…]");
                return 2;
            };
            let Some(command) = plugin.commands().into_iter().find(|c| c.name() == name) else {
                let _ = writeln!(err, "{}: no command {name}", plugin.name());
                return 2;
            };
            exit_code(command.run(args, out, err))
        }
        Some((first, _)) => {
            let _ = writeln!(
                err,
                "{}: {first:?} is no argument of a plugin's process: none answers the protocol on \
                 stdin and stdout, `command NAME ARGS…` runs a command",
                plugin.name()
            );
            2
        }
    }
}

/// A command's code as the exit status holds it: its low byte.
fn exit_code(code: i32) -> u8 {
    u8::try_from(code.rem_euclid(256)).unwrap_or(u8::MAX)
}

/// The protocol answered for `plugin`, its requests read from `input` and its answers written to
/// `output`, one line each, until `input` ends and every request read is answered. An error is
/// `input`'s, or a line longer than [`LINE_MAX`]: a line that is not an envelope is left
/// unanswered, as it names no call.
pub fn serve_on(
    plugin: &dyn Plugin,
    input: impl BufRead,
    output: impl Write + Send,
) -> io::Result<()> {
    serving(plugin, input, output, LINE_MAX)
}

/// [`serve_on`] with lines of at most `max` bytes both ways.
fn serving(
    plugin: &dyn Plugin,
    mut input: impl BufRead,
    output: impl Write + Send,
    max: usize,
) -> io::Result<()> {
    let output = Mutex::new(output);
    let pages = Pages::default();
    let (asked, jobs) = mpsc::channel::<(u64, Box<RawValue>)>();
    let jobs = Mutex::new(jobs);
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let job = jobs.lock().unwrap_or_else(PoisonError::into_inner).recv();
                    let Ok((id, request)) = job else {
                        break;
                    };
                    let started = Instant::now();
                    let answered = answered(plugin, request.get(), &pages);
                    let us = (started.elapsed().as_secs_f64() * 1e9).round() / 1e3;
                    write(&output, id, &answered, Some(us), max);
                }
            });
        }
        let read = loop {
            let line = match read_line(&mut input, max) {
                Ok(Lined::Line(line)) => line,
                Ok(Lined::End) => break Ok(()),
                Ok(Lined::TooLong(_)) => {
                    break Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("a line longer than the protocol's {max} bytes"),
                    ));
                }
                Err(e) => break Err(e),
            };
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let Ok(envelope) = serde_json::from_slice::<Raw>(&line) else {
                continue;
            };
            let id = envelope.id;
            match envelope.member() {
                Ok(Member::Request(request)) => {
                    let _ = asked.send((id, request));
                }
                Ok(Member::Cancel | Member::Answered(_)) => {}
                Err(why) => write(&output, id, &refused(&why), None, max),
            }
        };
        drop(asked);
        read
    })
}

/// The line answering the call `id` with `answered`, written to `output` whole; an answer that
/// would make a line longer than `max` is refused instead, naming its length.
fn write(output: &Mutex<impl Write>, id: u64, answered: &RawValue, us: Option<f64>, max: usize) {
    let line = |answered: &RawValue| {
        serde_json::to_vec(&Envelope::<(), &RawValue>::answering(id, answered, us)).ok()
    };
    let Some(mut line) = line(answered).and_then(|whole| {
        if whole.len() <= max {
            return Some(whole);
        }
        let why = format!(
            "its answer is a line of {} bytes, longer than the protocol's {max}",
            whole.len()
        );
        line(&refused(&why))
    }) else {
        return;
    };
    line.push(b'\n');
    let mut output = output.lock().unwrap_or_else(PoisonError::into_inner);
    // A host gone stops reading: its input ends next, and with it this process.
    let _ = output.write_all(&line).and_then(|()| output.flush());
}

/// `why` as the refusal the host shows in the call's place.
fn refused(why: &str) -> Box<RawValue> {
    raw(&Answered::<()>::Refused(why.to_string())).unwrap_or_else(|_| {
        RawValue::from_string("{\"refused\":\"\"}".into()).expect("an empty refusal is JSON")
    })
}

fn raw<T: Serialize>(value: &T) -> Result<Box<RawValue>, String> {
    serde_json::to_string(value)
        .and_then(RawValue::from_string)
        .map_err(|e| e.to_string())
}

/// What answers `request`, written: the call's answer, or why it is refused.
pub(crate) fn answered(plugin: &dyn Plugin, request: &str, pages: &Pages) -> Box<RawValue> {
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let request = serde_json::from_str::<Request>(request).map_err(|e| e.to_string())?;
        answer(plugin, request, pages)
    }));
    match answer {
        Ok(Ok(answered)) => answered,
        Ok(Err(why)) => refused(&why),
        Err(panic) => {
            let said = panic
                .downcast_ref::<&str>()
                .map(|said| (*said).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            refused(&format!("{} panicked: {said}", plugin.name()))
        }
    }
}

/// `answer` as its call answers it on the wire.
fn reply<T: Serialize>(answer: &T) -> Result<Box<RawValue>, String> {
    raw(&Answered::Answer(answer))
}

/// The object of `seam` called `name` among `objects`, or why there is none.
fn named<'p, T: ?Sized>(
    objects: Vec<&'p T>,
    seam: &str,
    name: &str,
    called: impl Fn(&T) -> &str,
) -> Result<&'p T, String> {
    objects
        .into_iter()
        .find(|object| called(object) == name)
        .ok_or_else(|| format!("the plugin has no {seam} {name:?}"))
}

/// The call `request` made of `plugin`, its answer written.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per call of the protocol, each a lookup and a call"
)]
fn answer(plugin: &dyn Plugin, request: Request, pages: &Pages) -> Result<Box<RawValue>, String> {
    match request {
        Request::Declare(_) => reply(&Declaration::of(plugin)),
        Request::Render(asked) => {
            let component = named(plugin.components(), "component", &asked.component, |c| {
                c.name()
            })?;
            reply(&pages.with(&asked.page, |page| {
                component.render(page, &asked.argument, asked.option.as_deref())
            }))
        }
        Request::Resolve(asked) => {
            let component = named(plugin.components(), "component", &asked.component, |c| {
                c.name()
            })?;
            reply(&pages.with(&asked.page, |page| {
                component.resolve(page, &asked.argument, asked.option.as_deref())
            }))
        }
        Request::CacheVersion(asked) => {
            let component = named(plugin.components(), "component", &asked.component, |c| {
                c.name()
            })?;
            reply(&pages.with(&asked.page, |page| component.cache_version(page)))
        }
        Request::Recognises(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.recognises(card)))
        }
        Request::Pictures(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.pictures(&asked.root, card)))
        }
        Request::Slot(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.slot(card)))
        }
        Request::TriageCategory(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.triage_category(card)))
        }
        Request::AutoApprovable(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.auto_approvable(card)))
        }
        Request::CardGates(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.gates(card)))
        }
        Request::ApprovalNeedsAgent(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.approval_needs_agent(card)))
        }
        Request::Chosen(asked) => {
            let kind = named(plugin.card_kinds(), "card kind", &asked.kind, |k| k.name())?;
            reply(&asked.card.read(|card| kind.chosen(card)))
        }
        Request::ReadCards(asked) => {
            let kinds = plugin.card_kinds();
            let read: Vec<Vec<KindAnswers>> = asked
                .cards
                .iter()
                .map(|card| {
                    card.read(|card| {
                        kinds
                            .iter()
                            .map(|kind| KindAnswers::of(*kind, &asked.root, card))
                            .collect()
                    })
                })
                .collect();
            reply(&read)
        }
        Request::Draw(asked) => {
            let renderer = named(plugin.renderers(), "renderer", &asked.renderer, |r| {
                r.name()
            })?;
            reply(&pages.with(&asked.page, |page| renderer.draw(page, &asked.file.file())))
        }
        Request::Serve(asked) => {
            let route = named(plugin.routes(), "route", &asked.route, |r| r.path())?;
            reply(&route.serve(&asked.root, &asked.rest))
        }
        Request::ShowRegion(asked) => {
            let region = named(plugin.home_regions(), "home region", &asked.region, |r| {
                r.id()
            })?;
            reply(&region.region(&asked.root))
        }
        Request::Offer(asked) => {
            let action = named(plugin.board_actions(), "board action", &asked.action, |a| {
                a.name()
            })?;
            reply(&action.offer(&asked.root, &asked.task.task()))
        }
        Request::Submit(asked) => {
            let action = named(plugin.board_actions(), "board action", &asked.action, |a| {
                a.name()
            })?;
            reply(&action.submit(&asked.root, &asked.task.task(), &asked.submission))
        }
        Request::Validate(asked) => {
            let field = named(plugin.board_fields(), "board field", &asked.field, |f| {
                f.name()
            })?;
            reply(&field.validate(&asked.value))
        }
        Request::AnswerGates(asked) => {
            let gates = named(plugin.gates(), "gates answerer", &asked.answerer, |g| {
                g.name()
            })?;
            reply(&gates.answer(&asked.root, &asked.task.task(), asked.tip.as_deref()))
        }
        Request::KeptWhile(asked) => {
            let gates = named(plugin.gates(), "gates answerer", &asked.answerer, |g| {
                g.name()
            })?;
            reply(&gates.kept_while(&asked.root, &asked.task.task()))
        }
        Request::Attest(asked) => {
            let authority = named(plugin.authorities(), "authority", &asked.authority, |a| {
                a.adapter()
            })?;
            let within = std::time::Duration::from_millis(asked.within_ms);
            reply(&authority.attest(&asked.root, &asked.operation, within))
        }
        Request::Changed(asked) => {
            let watches = plugin.watches();
            let watch = watches
                .get(asked.watch)
                .ok_or_else(|| format!("the plugin has no watch {}", asked.watch))?;
            reply(&watch.changed(&asked.root))
        }
        Request::Forget(asked) => {
            let watches = plugin.watches();
            let watch = watches
                .get(asked.watch)
                .ok_or_else(|| format!("the plugin has no watch {}", asked.watch))?;
            watch.forget(&asked.root);
            reply(&())
        }
        Request::CheckDocs(asked) => {
            let check = named(plugin.doc_checks(), "doc check", &asked.check, |c| c.name())?;
            let problems: Vec<ProblemData> = check
                .check(&Docs::read(&asked.root))
                .iter()
                .map(ProblemData::of)
                .collect();
            reply(&problems)
        }
        Request::DocComponents(asked) => {
            let check = named(plugin.doc_checks(), "doc check", &asked.check, |c| c.name())?;
            reply(&check.components(&Docs::read(&asked.root)))
        }
        Request::CheckProse(asked) => {
            let rule = named(plugin.prose_rules(), "prose rule", &asked.rule, |r| {
                r.name()
            })?;
            reply(&rule.check(&asked.rel, &asked.text))
        }
        Request::RunCommand(asked) => Err(format!(
            "commands.run is not asked on a pipe: the command {} runs as this plugin's binary, \
             `command {} ARGS…`, with streams of its own",
            asked.command, asked.command
        )),
        Request::RegisterAgent(asked) => {
            let transport = named(plugin.transports(), "transport", &asked.transport, |t| {
                t.name()
            })?;
            reply(&transport.register(&asked.handle, &asked.reported))
        }
        Request::Deliver(asked) => {
            let transport = named(plugin.transports(), "transport", &asked.transport, |t| {
                t.name()
            })?;
            reply(&transport.deliver(&asked.address, &asked.message))
        }
        Request::Alive(asked) => {
            let transport = named(plugin.transports(), "transport", &asked.transport, |t| {
                t.name()
            })?;
            reply(&transport.alive(&asked.address))
        }
    }
}

/// The pages of the renders a process answers for, the newest [`KEPT_RENDERS`]: what their
/// components have loaded, by render.
#[derive(Default)]
pub(crate) struct Pages {
    kept: Mutex<VecDeque<(u64, crate::Loaded)>>,
}

impl Pages {
    /// What `run` makes of the page `data` names: the page of its render, sharing what that render's
    /// earlier requests loaded, or a page of its own for a render of 0.
    fn with<R>(&self, data: &PageData, run: impl FnOnce(&Page<'_>) -> R) -> R {
        if data.render == 0 {
            return run(&Page::new(&data.root, &data.doc_path).in_artifacts(&data.artifacts));
        }
        let loaded = {
            let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
            match kept.iter().find(|(render, _)| *render == data.render) {
                Some((_, loaded)) => loaded.clone(),
                None => {
                    let loaded = crate::Loaded::default();
                    kept.push_back((data.render, loaded.clone()));
                    if kept.len() > KEPT_RENDERS {
                        kept.pop_front();
                    }
                    loaded
                }
            }
        };
        run(&Page::sharing(&data.root, &data.doc_path, loaded).in_artifacts(&data.artifacts))
    }
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
