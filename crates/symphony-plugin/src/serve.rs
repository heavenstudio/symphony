//! A plugin answering from a process of its own: its binary's `main` is [`serve`]`(&PLUGIN)`, and
//! the host that starts it speaks the protocol on its stdin and stdout (docs/PLUGIN_PROTOCOL.md).
//!
//! Each line read is an [`Envelope`]; a request is answered on one of [`WORKERS`] threads, so a slow
//! call holds one worker and the others go on answering, and answers may come back in another order
//! than their requests. A request that does not read, a call this plugin has no object for, or a
//! panic in the call is answered `{"refused": …}` naming why, by the request's id. A cancel is read
//! and changes nothing: a call runs to its end and its answer is written, which the host drops. The
//! process ends when its stdin does, once every call read is answered.

use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Instant;

use serde::Serialize;
use serde_json::value::RawValue;

use crate::protocol::{
    Answered, Declaration, Envelope, Member, PageData, ProblemData, Raw, Request,
};
use crate::{Docs, Page, Plugin};

/// The calls a process answers at once.
pub const WORKERS: usize = 4;

/// The renders whose pages a process keeps, the newest: a render's components share their `once`
/// values while its page is among them ([`PageData::render`]).
const KEPT_RENDERS: usize = 32;

/// The plugin's process: with no argument it answers the protocol on stdin and stdout until stdin
/// ends; `command NAME ARGS…` runs the plugin's command `NAME` with `ARGS`, writing to this
/// process's own stdout and stderr, and exits with its code, as `symphony NAME ARGS…` runs it.
pub fn serve(plugin: &dyn Plugin) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(main(
        plugin,
        &args,
        io::stdin().lock(),
        &mut io::stdout(),
        &mut io::stderr(),
    ))
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
/// `input`'s: a line that is not an envelope is left unanswered, as it names no call.
pub fn serve_on(
    plugin: &dyn Plugin,
    input: impl BufRead,
    output: impl Write + Send,
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
                    write(&output, id, &answered, Some(us));
                }
            });
        }
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let Ok(envelope) = serde_json::from_str::<Raw>(&line) else {
                continue;
            };
            let id = envelope.id;
            match envelope.member() {
                Ok(Member::Request(request)) => {
                    let _ = asked.send((id, request));
                }
                Ok(Member::Cancel | Member::Answered(_)) => {}
                Err(why) => write(&output, id, &refused(&why), None),
            }
        }
        drop(asked);
        Ok(())
    })
}

/// The line answering the call `id` with `answered`, written to `output` whole.
fn write(output: &Mutex<impl Write>, id: u64, answered: &RawValue, us: Option<f64>) {
    let Ok(mut line) =
        serde_json::to_string(&Envelope::<(), &RawValue>::answering(id, answered, us))
    else {
        return;
    };
    line.push('\n');
    let mut output = output.lock().unwrap_or_else(PoisonError::into_inner);
    // A host gone stops reading: its input ends next, and with it this process.
    let _ = output
        .write_all(line.as_bytes())
        .and_then(|()| output.flush());
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
fn answered(plugin: &dyn Plugin, request: &str, pages: &Pages) -> Box<RawValue> {
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
struct Pages {
    kept: Mutex<VecDeque<(u64, crate::Loaded)>>,
}

impl Pages {
    /// What `run` makes of the page `data` names: the page of its render, sharing what that render's
    /// earlier requests loaded, or a page of its own for a render of 0.
    fn with<R>(&self, data: &PageData, run: impl FnOnce(&Page<'_>) -> R) -> R {
        if data.render == 0 {
            return run(&Page::new(&data.root, &data.doc_path));
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
        run(&Page::sharing(&data.root, &data.doc_path, loaded))
    }
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
