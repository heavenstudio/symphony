//! `symphony-github COMMAND`: the adapter's whole surface, each command an operator's own act.
//!
//! `status` reads what is kept and the committed settings and writes nothing anywhere, and so
//! does `describe`, which says it as the exchange's entry on the Settings page
//! ([`crate::describe`]); `tokens` says of each token file whether its token is current, never the
//! token.
//! `publish`, `resolve` and `drop` write only the adapter's own folder, and `resolve` and `drop`
//! make none where nothing is kept. `preview` reads GitHub when the adapter is turned on and
//! writes nothing there or on the board; turned off, it reads nothing there, and neither it nor
//! `reconcile` makes the folder. `reconcile` is the one command that writes either, and only the
//! preview it is handed. `deliver` is the land's, not an operator's: it answers one verb of a
//! delivery by pull request ([`crate::deliver`]). `release` is the release run's: a GitHub
//! Release for a tag it pushed ([`crate::release`]), and `verify` its commit's verification on
//! GitHub's machines ([`crate::verify`]).

use crate::config::{PullRequests, Releases, Verification};
use crate::github::{clean_body, clean_title};
use crate::mint::{self, Held};
use crate::plan;
use crate::reconcile::{self, Context, Outcome};
use crate::state::{Draft, Entry, Event, Flag, Origin, Side, State, Store};
use serde_json::{Value, json};
use std::io::Write;

/// What `--help` and a command line that does not read are answered with.
pub const USAGE: &str = "\
usage: symphony-github [--tasks PROGRAM] COMMAND

The optional exchange of selected tasks with GitHub issues. It is turned off unless the
project's default branch commits symphony-github.toml with enabled = true.

  status                              what is kept and what the committed settings allow;
                                      reads no remote and writes nothing
  describe                            what status says, as the exchange's entry on the
                                      Settings page: one line of JSON, which the server runs
                                      this to read; reads no remote and writes nothing
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
  drop SLUG                           drop SLUG's mapping and keep its issue as not SLUG's: no
                                      preview maps the two again, and a curated text kept is
                                      published anew
  deliver VERB --request FILE         answer a land delivered by a pull request, which runs it
                                      (`[land] delivery`): publish, merge, result or landed
  release --request FILE              answer the release run with a GitHub Release for a tag it
                                      pushed (`[releases] github`); edits and deletes none
  verify --request FILE               answer the release run with its commit's verification on
                                      GitHub's machines (`[releases] hosted`): starts the
                                      workflow, waits for it and downloads its logs
  preflight [--branch BASE]           read the protection of BASE (the default branch) as a
                                      delivery does before it writes, and say what is missing;
                                      writes nothing anywhere but a token it makes
  rulesets [--verify] [--branch BASE] read the rulesets on BASE (the default branch) in full with
                                      gh, signed in as someone who administers the repository,
                                      and judge them; --verify keeps who bypasses each, which a
                                      delivery takes while the ruleset is unchanged
  tokens                              whether the token is current and when it expires, never
                                      the token; reads no remote and writes nothing

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
            let _ = writeln!(
                out,
                "releases: {}{}",
                config.releases.word(),
                match config.releases {
                    Releases::Off => "",
                    Releases::Publish => " (a release run's GitHub Release: `release`)",
                }
            );
            if config.verification == Verification::Hosted {
                let _ = writeln!(
                    out,
                    "verification: hosted (a release run's checks on GitHub's machines: `verify`)"
                );
            }
            if let Some(file) = &config.token_file {
                let _ = writeln!(out, "token file: {}", file.display());
            }
            if let Some(id) = config.app_id {
                let _ = writeln!(out, "app: {id}");
            }
        }
        Err(why) => {
            let _ = writeln!(out, "turned off: its settings do not read: {why}");
        }
    }
    kept(ctx, out)
}

/// `tokens`: for the token file the committed settings name, whether its token is current and
/// when it expires, or what makes one; never the token. It exits 0 unless the file is refused.
fn tokens(ctx: &Context<'_>, out: &mut dyn Write) -> i32 {
    let config = match ctx.source.load() {
        Ok(loaded) => loaded.config,
        Err(why) => {
            let _ = writeln!(out, "turned off: its settings do not read: {why}");
            return 1;
        }
    };
    let mut code = 0;
    let Some(file) = config.token_file.as_deref() else {
        let _ = writeln!(out, "the committed settings name no token file");
        return code;
    };
    let next = |id: i64, what: &str| format!("App {id} makes {what} at its next use");
    let said = match (mint::standing(ctx, &config, file), config.app()) {
        (Err(why), _) => {
            code = 1;
            format!("refused: {why}")
        }
        (Ok(_), None) => "kept by hand: when it expires is not known here".to_string(),
        (Ok(Held::Unknown), Some(app)) => format!(
            "holds no expiry this program wrote; {}",
            next(app.id, "another")
        ),
        (Ok(Held::Absent), Some(app)) => format!("absent; {}", next(app.id, "one")),
        (
            Ok(Held::Token {
                expires,
                permissions,
                ..
            }),
            Some(app),
        ) => {
            let left = expires.at - ctx.now;
            let asked = mint::asked(&config);
            if left > mint::MARGIN && permissions.as_deref() == Some(asked.as_str()) {
                format!("current until {} (in {} min)", expires.text, left / 60)
            } else if left > mint::MARGIN {
                let made = permissions.map_or_else(
                    || "permissions it does not record".to_string(),
                    |made| format!("permissions {made}"),
                );
                format!(
                    "current until {} (in {} min), made for {made} where the settings ask for \
                     {asked}; {}",
                    expires.text,
                    left / 60,
                    next(app.id, "another")
                )
            } else if left > 0 {
                format!(
                    "expires at {} (in {} min); {}",
                    expires.text,
                    left / 60,
                    next(app.id, "another")
                )
            } else {
                format!("expired at {}; {}", expires.text, next(app.id, "another"))
            }
        }
    };
    let _ = writeln!(out, "{} {}: {said}", mint::TOKEN_FILE, file.display());
    code
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
    for (tag, released) in &state.releases {
        let _ = writeln!(out, "  {tag} is published as {}", released.url);
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
    for (slug, issues) in &state.dropped {
        let _ = writeln!(
            out,
            "  {slug}: {} issue(s) dropped from it, never mapped to it again",
            issues.len()
        );
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
    let longest = plan::longest_body();
    if body.chars().count() > longest {
        return Err(format!(
            "{file}: the public text is longer than {longest} characters, which is all an issue \
             holds whole once it ends in its marker's line: shorten it"
        ));
    }
    let tasks = ctx.board.tasks().map_err(|e| e.said().to_string())?;
    if !tasks.iter().any(|task| task.slug == *slug) {
        return Err(format!("no task {slug} is on the board"));
    }
    let store = Store::open(&ctx.board.state_folder().map_err(|e| e.said().to_string())?)?;
    let _lock = store.lock()?;
    let mut state = store.state()?.unwrap_or_default();
    if let Some(mapping) = (state.mappings.get(slug)).filter(|m| m.origin == Origin::Import) {
        return Err(format!(
            "{slug} is imported from issue #{}: its text is its brief, which that issue's edits \
             reach, and a curated text of it would be published nowhere",
            mapping.issue_number
        ));
    }
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
    let folder = ctx.board.state_folder().map_err(|e| e.said().to_string())?;
    let Some(store) = Store::existing(&folder) else {
        return Err(format!("{slug} is mapped to no issue: nothing is kept"));
    };
    let _lock = store.lock()?;
    let mut state: State = store.state()?.unwrap_or_default();
    let Some(mapping) = state.mappings.get_mut(slug) else {
        return Err(format!("{slug} is mapped to no issue"));
    };
    // A preview read before the side was kept is never applied after it, so it goes before
    // anything is written: a stop between the two leaves no preview, never an old one.
    store.drop_preview()?;
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

/// `drop SLUG`: the task's mapping is dropped and its issue kept as not the task's, so no
/// preview maps the two again or takes that issue for the task's publication; a curated text
/// stays, and the next preview publishes it anew. It writes only the adapter's own folder: the
/// preview waiting is dropped first, then the drop is an act of its own in the journal, its
/// intent holding the mapping it drops, then its acknowledgment.
fn drop_mapping(ctx: &Context<'_>, args: &[String], out: &mut dyn Write) -> Outcome {
    let [slug] = args else {
        return Err("drop takes one SLUG".into());
    };
    let folder = ctx.board.state_folder().map_err(|e| e.said().to_string())?;
    let Some(store) = Store::existing(&folder) else {
        return Err(format!("{slug} is mapped to no issue: nothing is kept"));
    };
    let _lock = store.lock()?;
    let mut state: State = store.state()?.unwrap_or_default();
    let Some(mapping) = state.mappings.get(slug).cloned() else {
        return Err(format!("{slug} is mapped to no issue"));
    };
    let entry = |event: Event, result: Value| Entry {
        at: ctx.now,
        event,
        preview: String::new(),
        op: format!("drop:{slug}:{}", mapping.issue_id),
        kind: "drop".into(),
        target: slug.clone(),
        digest: String::new(),
        base: serde_json::to_value(&mapping).unwrap_or_default(),
        config: String::new(),
        by: String::new(),
        result,
    };
    // A preview read before the drop would adopt the mapping again, or push the curated text
    // into the issue dropped, so it goes before anything is written: a stop between the writes
    // leaves no preview, never an old one.
    store.drop_preview()?;
    store.journal(&entry(Event::Intent, Value::Null))?;
    state.mappings.remove(slug);
    state.resolutions.remove(slug);
    (state.dropped.entry(slug.clone()).or_default()).insert(mapping.issue_id);
    store.save(&state)?;
    store.journal(&entry(Event::Ack, json!({"dropped": mapping.issue_id})))?;
    let _ = writeln!(
        out,
        "{slug} is no longer mapped to issue #{} ({}), which is kept as not this task's. Nothing \
         was written to it or to the board.",
        mapping.issue_number, mapping.url
    );
    if state.drafts.contains_key(slug) {
        let _ = writeln!(
            out,
            "Its curated text stays: the next preview shows it published as a new issue."
        );
    }
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
        "describe" => {
            none_left(&rest)?;
            let entry = crate::describe::entry(ctx);
            let text = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
            let _ = writeln!(out, "{text}");
            Ok(0)
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
        "drop" => drop_mapping(ctx, &rest, out),
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
        "release" => {
            let file = need(&mut rest, "--request")?;
            none_left(&rest)?;
            crate::release::run(ctx, std::path::Path::new(&file), out)
        }
        "verify" => {
            let file = need(&mut rest, "--request")?;
            none_left(&rest)?;
            crate::verify::run(ctx, std::path::Path::new(&file), out)
        }
        "preflight" => {
            let named = option(&mut rest, "--branch")?;
            none_left(&rest)?;
            let base = match named {
                Some(base) => base,
                None => crate::config::default_branch(
                    ctx.repository
                        .first()
                        .ok_or("the project has no main checkout")?,
                )?,
            };
            crate::deliver::preflight(ctx, &base, out)
        }
        "rulesets" => {
            let verify = flag(&mut rest, "--verify");
            let named = option(&mut rest, "--branch")?;
            none_left(&rest)?;
            let base = match named {
                Some(base) => base,
                None => crate::config::default_branch(
                    ctx.repository
                        .first()
                        .ok_or("the project has no main checkout")?,
                )?,
            };
            if !crate::deliver::plain(&base, "-._/") {
                return Err(format!("no branch is named {base:?}"));
            }
            crate::rulesets::run(ctx, &base, verify, out)
        }
        "tokens" => {
            none_left(&rest)?;
            Ok(tokens(ctx, out))
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
