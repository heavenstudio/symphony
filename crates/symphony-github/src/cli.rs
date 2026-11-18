//! `symphony-github COMMAND`: the adapter's whole surface, each command an operator's own act.
//!
//! `status` reads what is kept and the committed settings and writes nothing anywhere.
//! `publish` and `resolve` write only the adapter's own folder. `preview` reads GitHub when the
//! adapter is turned on and writes nothing there or on the board. `reconcile` is the one
//! command that writes either, and only the preview it is handed. `deliver` is the land's, not an
//! operator's: it answers one verb of a delivery by pull request ([`crate::deliver`]).

use crate::config::PullRequests;
use crate::github::{clean_body, clean_title};
use crate::reconcile::{self, Context, Outcome};
use crate::state::{Draft, Event, Flag, Side, State, Store};
use std::io::Write;

/// What `--help` and a command line that does not read are answered with.
pub const USAGE: &str = "\
usage: symphony-github [--tasks PROGRAM] COMMAND

The optional exchange of selected tasks with GitHub issues. It is turned off unless the
project's default branch commits symphony-github.toml with enabled = true.

  status                              what is kept and what the committed settings allow;
                                      reads no remote and writes nothing
  publish SLUG --title TITLE --body-file FILE
                                      opt a task in: keep TITLE and FILE's text as its curated
                                      public text, shown as it will be published
  preview [--rebind]                  read both sides and show exactly what a reconcile would
                                      write; writes nothing to GitHub or the board
  reconcile --preview DIGEST --as HANDLE [--limit N]
                                      apply the preview of that digest, N operations at most
                                      (50); safe to run again
  resolve SLUG local|remote           keep one side of a conflict, or take up a suspended
                                      mapping again; the next preview shows what follows
  deliver VERB --request FILE         answer a land delivered by a pull request, which runs it
                                      (`[land] delivery`): publish, merge, result or landed

--tasks PROGRAM names the board's program, else the `tasks` beside this one or on PATH.
";

/// The operations one `reconcile` makes unless `--limit` says another number.
const LIMIT: usize = 50;

/// `--help` and `--version`, which need no project: said on `out`, and true.
pub fn answered_without_a_project(args: &[String], out: &mut dyn Write) -> bool {
    match args {
        [only] if only == "--version" => {
            let _ = writeln!(out, "symphony-github {}", env!("CARGO_PKG_VERSION"));
            true
        }
        [] => {
            let _ = write!(out, "{USAGE}");
            true
        }
        [first, ..] if first == "--help" || first == "-h" || first == "help" => {
            let _ = write!(out, "{USAGE}");
            true
        }
        _ => false,
    }
}

/// The value after the option `name`, taken out of `args`.
fn option(args: &mut Vec<String>, name: &str) -> Result<Option<String>, String> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    if at + 1 >= args.len() {
        return Err(format!("{name} needs a value"));
    }
    args.remove(at);
    Ok(Some(args.remove(at)))
}

fn flag(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|arg| arg != name);
    args.len() != before
}

fn need(args: &mut Vec<String>, name: &str) -> Result<String, String> {
    option(args, name)?.ok_or_else(|| format!("{name} is needed"))
}

fn none_left(args: &[String]) -> Result<(), String> {
    match args.first() {
        Some(extra) => Err(format!("unexpected argument {extra:?}")),
        None => Ok(()),
    }
}

fn status(ctx: &Context<'_>, out: &mut dyn Write) -> Outcome {
    match ctx.source.load() {
        Ok(loaded) => {
            let config = &loaded.config;
            let _ = writeln!(
                out,
                "{}",
                if config.enabled {
                    "enabled by the committed settings"
                } else {
                    "turned off: nothing is read from or written to GitHub"
                }
            );
            if let Some(repository) = &config.repository {
                let _ = writeln!(out, "repository: {}", repository.full_name());
            }
            let _ = writeln!(out, "issues: {}", config.issues.word());
            let _ = writeln!(
                out,
                "pull requests: {}{}",
                config.pull_requests.word(),
                match config.pull_requests {
                    PullRequests::Off => "",
                    PullRequests::Track => " (this build tracks none)",
                    PullRequests::Publish => " (a land's delivery: `deliver`)",
                }
            );
            if let Some(file) = &config.token_file {
                let _ = writeln!(out, "token file: {}", file.display());
            }
        }
        Err(why) => {
            let _ = writeln!(out, "turned off: its settings do not read: {why}");
        }
    }
    kept(ctx, out)
}

/// What `status` says of the state folder, reading it and nothing else.
fn kept(ctx: &Context<'_>, out: &mut dyn Write) -> Outcome {
    let folder = ctx.board.state_folder().map_err(|e| e.said().to_string())?;
    let Some(store) = Store::existing(&folder) else {
        let _ = writeln!(out, "nothing is kept: {} is not there", folder.display());
        return Ok(0);
    };
    let state = store.state()?.unwrap_or_default();
    match &state.binding {
        Some(binding) => {
            let _ = writeln!(
                out,
                "bound to {} (id {})",
                binding.full_name, binding.repository_id
            );
        }
        None => {
            let _ = writeln!(out, "bound to no repository yet");
        }
    }
    if let Some(why) = &state.paused {
        let _ = writeln!(out, "paused: {why}");
    }
    if let Some(until) = state.rate_limited_until.filter(|until| *until > ctx.now) {
        let _ = writeln!(out, "rate limited until {until} (seconds since 1970)");
    }
    let _ = writeln!(
        out,
        "{} mapped, {} curated texts",
        state.mappings.len(),
        state.drafts.len()
    );
    for (slug, pull) in &state.pulls {
        let landed = (pull.landed.as_deref())
            .map(|squash| {
                format!(
                    " (landed as {})",
                    squash.chars().take(12).collect::<String>()
                )
            })
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "  {slug} is delivered by #{} {}{landed}",
            pull.number, pull.url
        );
    }
    for (slug, mapping) in &state.mappings {
        let flag = mapping
            .flag
            .as_ref()
            .map(|flag| format!(" [{}]", flag.said()))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "  {slug} <-> #{} {} ({}){flag}",
            mapping.issue_number, mapping.url, mapping.remote_state
        );
    }
    for slug in state
        .drafts
        .keys()
        .filter(|slug| !state.mappings.contains_key(*slug))
    {
        let _ = writeln!(out, "  {slug}: curated text waits to be published");
    }
    if let Some(preview) = store.preview()? {
        let _ = writeln!(
            out,
            "preview {} waits: {} operations",
            preview.digest,
            preview.actions.len()
        );
    }
    let entries = store.entries()?;
    for entry in entries.iter().rev().take(5).rev() {
        let event = match entry.event {
            Event::Intent => "intent",
            Event::Ack => "done",
            Event::Skipped => "skipped",
            Event::Failed => "not made",
            Event::Unknown => "result unknown",
            Event::Applied => "preview applied",
        };
        let _ = writeln!(
            out,
            "journal: {} {} {} {event}",
            entry.at, entry.kind, entry.target
        );
    }
    Ok(0)
}

fn publish(ctx: &Context<'_>, mut args: Vec<String>, out: &mut dyn Write) -> Outcome {
    let title = clean_title(&need(&mut args, "--title")?);
    let file = need(&mut args, "--body-file")?;
    let [slug] = args.as_slice() else {
        return Err("publish takes one SLUG".into());
    };
    if title.is_empty() {
        return Err("the public title is empty".into());
    }
    let body = std::fs::read_to_string(&file).map_err(|error| format!("{file}: {error}"))?;
    let body = clean_body(&body);
    let tasks = ctx.board.tasks().map_err(|e| e.said().to_string())?;
    if !tasks.iter().any(|task| task.slug == *slug) {
        return Err(format!("no task {slug} is on the board"));
    }
    let store = Store::open(&ctx.board.state_folder().map_err(|e| e.said().to_string())?)?;
    let _lock = store.lock()?;
    let mut state = store.state()?.unwrap_or_default();
    let _ = writeln!(
        out,
        "{slug}'s curated public text, kept here and sent nowhere yet:"
    );
    let _ = writeln!(out, "    public title: {title}");
    for line in body.lines() {
        let _ = writeln!(out, "    | {line}");
    }
    state.drafts.insert(slug.clone(), Draft { title, body });
    store.save(&state)?;
    let _ = writeln!(
        out,
        "`symphony-github preview` shows it among what a reconcile would write."
    );
    Ok(0)
}

fn resolve(ctx: &Context<'_>, args: &[String], out: &mut dyn Write) -> Outcome {
    let [slug, side] = args else {
        return Err("resolve takes SLUG and local or remote".into());
    };
    let side = match side.as_str() {
        "local" => Side::Local,
        "remote" => Side::Remote,
        other => return Err(format!("keep local or remote, not {other:?}")),
    };
    let store = Store::open(&ctx.board.state_folder().map_err(|e| e.said().to_string())?)?;
    let _lock = store.lock()?;
    let mut state: State = store.state()?.unwrap_or_default();
    let Some(mapping) = state.mappings.get_mut(slug) else {
        return Err(format!("{slug} is mapped to no issue"));
    };
    if matches!(mapping.flag, Some(Flag::Suspended(_))) {
        // Taken up again for the task the slug names now, under the board's generation now,
        // with digests neither side has: the side kept decides what the next preview writes.
        let bound = ctx.board.bound(slug).map_err(|e| e.said().to_string())?;
        mapping.task_id = bound.token.id();
        mapping.generation = bound.token.generation().into();
        mapping.local = "unknown".into();
        mapping.remote = "unknown".into();
        mapping.flag = None;
    }
    state.resolutions.insert(slug.clone(), side);
    store.save(&state)?;
    store.drop_preview()?;
    let _ = writeln!(
        out,
        "{slug}: the {} side is kept. Nothing was written yet: preview, then reconcile.",
        if side == Side::Local {
            "local"
        } else {
            "remote"
        }
    );
    Ok(0)
}

fn command(ctx: &Context<'_>, args: &[String], out: &mut dyn Write) -> Outcome {
    let (name, rest) = args.split_first().ok_or("a command is needed")?;
    let mut rest = rest.to_vec();
    match name.as_str() {
        "status" => {
            none_left(&rest)?;
            status(ctx, out)
        }
        "publish" => publish(ctx, rest, out),
        "preview" => {
            let rebind = flag(&mut rest, "--rebind");
            none_left(&rest)?;
            reconcile::preview(ctx, rebind, out)
        }
        "reconcile" => {
            let digest = need(&mut rest, "--preview")?;
            let by = need(&mut rest, "--as")?;
            let limit = match option(&mut rest, "--limit")? {
                Some(limit) => limit
                    .parse::<usize>()
                    .ok()
                    .filter(|limit| *limit > 0)
                    .ok_or("--limit is a number of operations, 1 or more")?,
                None => LIMIT,
            };
            none_left(&rest)?;
            reconcile::apply(ctx, &digest, &by, limit, out)
        }
        "resolve" => resolve(ctx, &rest, out),
        "deliver" => {
            let file = need(&mut rest, "--request")?;
            let [verb] = rest.as_slice() else {
                return Err("deliver takes one VERB: publish, merge, result or landed".into());
            };
            let main = ctx
                .repository
                .first()
                .ok_or("the project has no main checkout")?;
            let remote = crate::deliver::Git { main: main.clone() };
            crate::deliver::run(ctx, &remote, verb, std::path::Path::new(&file), out)
        }
        other => Err(format!("no command {other:?}\n{USAGE}")),
    }
}

/// `symphony-github ARGS`: what it says goes to `out`, a refusal to `err`; its exit code.
pub fn run(args: &[String], ctx: &Context<'_>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if answered_without_a_project(args, out) {
        return 0;
    }
    match command(ctx, args, out) {
        Ok(code) => code,
        Err(why) => {
            let _ = writeln!(err, "symphony-github: {why}");
            1
        }
    }
}
