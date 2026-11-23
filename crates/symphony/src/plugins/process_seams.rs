//! The seams a plugin's process answers besides its components and card kinds: its renderers,
//! routes, home regions, board actions, gates answerers, approval authorities, Settings page
//! describers, watches and doc checks, each the object its declaration names, forwarding each
//! call. What the declaration says of an object (a renderer's extensions and content type, a
//! watch's paths, a check's inputs) is answered from it, with no call. A call that gets no answer
//! is shown where its answer would go, as the seam has room for it: a renderer's drawing as its
//! error in the file's place, a route's answer as not found and a home region as none (each
//! failure in the plugin's log), a board action's offer or submission as its refusal, a gates
//! answer and what it is kept while as the answerer's error ([`RemoteGates`]), a describer's entry
//! as its error in the entry's place, a watch's warming as the server's log line saying so, a doc
//! check's as a problem naming it, and its components as none. Its commands are no call: each runs
//! as the plugin's program, a process of its own ([`RemoteCommand`]).

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use symphony_plugin::authority::{Operation, Signed};
use symphony_plugin::protocol::{
    AnswerGates, Ask, Attest, Changed, CheckDocs, Describe, DocComponents, Draw, FileData, Forget,
    KeptWhile, Offer, PageData, ProblemData, Serve, ShowRegion, Submit, TaskData,
};
use symphony_plugin::vocabulary::{Gate, Region, SettingsEntry, TaskOffer};
use symphony_plugin::{
    Authority, BoardAction, Command, Describer, DocCheck, Docs, Drawing, File, Gates, HomeRegion,
    Page, Problem, Renderer, Reply, Route, Submission, Submitted, TaskRef, Watch,
};

use super::{Failure, Remote};

/// A renderer the remote declared.
pub(super) struct RemoteRenderer {
    pub(super) remote: &'static Remote,
    pub(super) name: String,
    pub(super) extensions: Vec<&'static str>,
    pub(super) content_type: String,
}

impl Renderer for RemoteRenderer {
    fn name(&self) -> &str {
        &self.name
    }
    fn extensions(&self) -> &[&str] {
        &self.extensions
    }
    fn content_type(&self) -> &str {
        &self.content_type
    }
    fn draw(&self, page: &Page<'_>, file: &File<'_>) -> Result<Drawing, String> {
        let asked = Draw::new(self.name.clone(), PageData::of(page), FileData::of(file));
        match self.remote.ask(&self.name, asked) {
            Ok(drawn) => drawn,
            // Required of every renderer, so in every minor.
            Err(Failure::Newer(agreed)) => Err(self.remote.failed(&format!(
                "it speaks protocol {agreed}, which has no {}",
                Draw::TAG
            ))),
            Err(Failure::Down(why)) => Err(self.remote.failed(&why)),
        }
    }
}

/// A route the remote declared, named by its path.
pub(super) struct RemoteRoute {
    pub(super) remote: &'static Remote,
    pub(super) path: String,
}

impl Route for RemoteRoute {
    fn path(&self) -> &str {
        &self.path
    }
    fn serve(&self, root: &Path, rest: &str) -> Option<Reply> {
        let asked = Serve::new(self.path.clone(), root.to_path_buf(), rest.into());
        // No answer is not found: why is in the plugin's log.
        self.remote.ask(&self.path, asked).ok().flatten()
    }
}

/// A home region the remote declared, named by its id.
pub(super) struct RemoteRegion {
    pub(super) remote: &'static Remote,
    pub(super) id: String,
}

impl HomeRegion for RemoteRegion {
    fn id(&self) -> &str {
        &self.id
    }
    fn region(&self, root: &Path) -> Option<Region> {
        let asked = ShowRegion::new(self.id.clone(), root.to_path_buf());
        // No answer shows nothing: why is in the plugin's log.
        self.remote.ask(&self.id, asked).ok().flatten()
    }
}

/// A doc check the remote declared: the plugin reads the documents of the checkout the call names
/// itself, once for each call.
pub(super) struct RemoteCheck {
    pub(super) remote: &'static Remote,
    pub(super) name: String,
    pub(super) inputs: Vec<String>,
}

impl DocCheck for RemoteCheck {
    fn name(&self) -> &str {
        &self.name
    }
    fn components(&self, docs: &Docs) -> Vec<String> {
        let asked = DocComponents::new(self.name.clone(), docs.root().to_path_buf());
        // None named: a component only it names reads unknown, so the gap is seen.
        self.remote.ask(&self.name, asked).unwrap_or_default()
    }
    fn inputs(&self) -> Vec<String> {
        self.inputs.clone()
    }
    fn check(&self, docs: &Docs) -> Vec<Problem> {
        let asked = CheckDocs::new(self.name.clone(), docs.root().to_path_buf());
        match self.remote.ask(&self.name, asked) {
            Ok(problems) => problems.into_iter().map(ProblemData::problem).collect(),
            Err(Failure::Newer(_)) => Vec::new(),
            // A check that did not run is a problem, never a pass.
            Err(Failure::Down(why)) => vec![Problem::new(
                crate::project::FILE,
                0,
                self.remote
                    .failed(&format!("its doc check {} did not run: {why}", self.name)),
            )],
        }
    }
}

/// A board action the remote declared.
pub(super) struct RemoteAction {
    pub(super) remote: &'static Remote,
    pub(super) name: String,
}

impl RemoteAction {
    /// `asked` of the remote: its answer, or why it has none as the action's error (a submission
    /// refused moves nothing).
    fn ask<A: Ask<Answer = Result<T, String>>, T>(&self, asked: A) -> Result<T, String> {
        match self.remote.ask(&self.name, asked) {
            Ok(answered) => answered,
            // Required of every action, so in every minor.
            Err(Failure::Newer(agreed)) => Err(self.remote.failed(&format!(
                "it speaks protocol {agreed}, which has no {}",
                A::TAG
            ))),
            Err(Failure::Down(why)) => Err(self.remote.failed(&why)),
        }
    }
}

impl BoardAction for RemoteAction {
    fn name(&self) -> &str {
        &self.name
    }
    fn offer(&self, root: &Path, task: &TaskRef<'_>) -> Result<TaskOffer, String> {
        self.ask(Offer::new(
            self.name.clone(),
            root.to_path_buf(),
            TaskData::of(task),
        ))
    }
    fn submit(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        submission: &Submission,
    ) -> Result<Submitted, String> {
        self.ask(Submit::new(
            self.name.clone(),
            root.to_path_buf(),
            TaskData::of(task),
            submission.clone(),
        ))
    }
}

/// A gates answerer the remote declared, named by its name. The Symphony asks it as it asks a
/// linked one ([`crate::gates`]): each answer kept by the task, its branch's tip and what
/// [`Gates::kept_while`] tells, on its own thread, for whatever checkout the call names. Its answer
/// is waited for as long as a linked one may run ([`super::Limits::gates`]), and asked only in its
/// turn among the remote's [`super::GATES_AT_ONCE`], so slow answers leave a worker of the plugin's
/// to its other calls.
pub(super) struct RemoteGates {
    pub(super) remote: &'static Remote,
    pub(super) name: String,
}

impl Gates for RemoteGates {
    fn name(&self) -> &str {
        &self.name
    }
    fn answer(
        &self,
        root: &Path,
        task: &TaskRef<'_>,
        tip: Option<&str>,
    ) -> Result<Vec<Gate>, String> {
        let _turn = self
            .remote
            .gates_turn()
            .map_err(|why| self.remote.failed(&why))?;
        let asked = AnswerGates::new(
            self.name.clone(),
            root.to_path_buf(),
            TaskData::of(task),
            tip.map(str::to_string),
        );
        match self
            .remote
            .ask_within(&self.name, asked, self.remote.limits.gates)
        {
            // Its gates, or its own error as it said it.
            Ok(answered) => answered,
            // Required of every answerer, so in every minor.
            Err(Failure::Newer(agreed)) => Err(self.remote.failed(&format!(
                "it speaks protocol {agreed}, which has no {}",
                AnswerGates::TAG
            ))),
            Err(Failure::Down(why)) => Err(self.remote.failed(&why)),
        }
    }
    /// What the plugin says its answer reads besides the task's status and tip. One with no answer
    /// has no error to return but a panic, which the gates read as the answerer's error, as a linked
    /// one's: unwound with its why ([`crate::gates::Untold`]) and without the panic hook, so a kept
    /// answer is not used past it and nothing is printed.
    fn kept_while(&self, root: &Path, task: &TaskRef<'_>) -> String {
        let asked = KeptWhile::new(self.name.clone(), root.to_path_buf(), TaskData::of(task));
        match self.remote.ask(&self.name, asked) {
            Ok(said) => said,
            // The trait's default: kept for the status and tip alone.
            Err(Failure::Newer(_)) => String::new(),
            Err(Failure::Down(why)) => {
                std::panic::resume_unwind(Box::new(crate::gates::Untold(self.remote.failed(&why))))
            }
        }
    }
}

/// An approval authority the remote declared, named by its adapter.
pub(super) struct RemoteAuthority {
    pub(super) remote: &'static Remote,
    pub(super) adapter: String,
}

impl Authority for RemoteAuthority {
    fn adapter(&self) -> &str {
        &self.adapter
    }
    fn attest(
        &self,
        root: &Path,
        operation: &Operation,
        within: Duration,
    ) -> Result<Signed, String> {
        let within_ms = u64::try_from(within.as_millis()).unwrap_or(u64::MAX);
        let asked = Attest::new(
            self.adapter.clone(),
            root.to_path_buf(),
            operation.clone(),
            within_ms,
        );
        match self.remote.ask(&self.adapter, asked) {
            Ok(answered) => answered,
            // Declared, so its minor has the call; an answer of none attests nothing.
            Err(Failure::Newer(agreed)) => Err(self.remote.failed(&format!(
                "it speaks protocol {agreed}, which has no {}",
                Attest::TAG
            ))),
            Err(Failure::Down(why)) => Err(self.remote.failed(&why)),
        }
    }
}

/// A Settings page describer the remote declared, named by its id.
pub(super) struct RemoteDescriber {
    pub(super) remote: &'static Remote,
    pub(super) id: String,
}

impl Describer for RemoteDescriber {
    fn id(&self) -> &str {
        &self.id
    }
    fn describe(&self, root: &Path) -> Result<SettingsEntry, String> {
        let asked = Describe::new(self.id.clone(), root.to_path_buf());
        match self.remote.ask(&self.id, asked) {
            Ok(answered) => answered,
            // Declared, so its minor has the call.
            Err(Failure::Newer(agreed)) => Err(self.remote.failed(&format!(
                "it speaks protocol {agreed}, which has no {}",
                Describe::TAG
            ))),
            Err(Failure::Down(why)) => Err(self.remote.failed(&why)),
        }
    }
}

/// A watch the remote declared: named on the wire by its place among the plugin's watches, and in
/// the record and the log by its paths.
pub(super) struct RemoteWatch {
    pub(super) remote: &'static Remote,
    pub(super) index: usize,
    pub(super) paths: Vec<String>,
}

impl RemoteWatch {
    fn named(&self) -> String {
        self.paths.join(", ")
    }
}

impl Watch for RemoteWatch {
    fn paths(&self) -> Vec<String> {
        self.paths.clone()
    }
    fn changed(&self, root: &Path) -> Option<String> {
        let asked = Changed::new(self.index, root.to_path_buf());
        match self.remote.ask(&self.named(), asked) {
            Ok(said) => said,
            Err(Failure::Newer(_)) => None,
            // The server's log says it did not warm.
            Err(Failure::Down(why)) => Some(
                self.remote
                    .failed(&format!("its watch of {} did not run: {why}", self.named())),
            ),
        }
    }
    fn forget(&self, root: &Path) {
        let asked = Forget::new(self.index, root.to_path_buf());
        // Nothing to show: a failure is in the plugin's log, and a process that ends forgets all
        // it kept.
        let _ = self.remote.ask(&self.named(), asked);
    }
}

/// A command the remote declared: never a call on the pipe, but the plugin's program run alone as
/// `PROGRAM ARGS… command NAME ARGS…`, as the SDK's `serve` runs one, with this process's own
/// streams (what was written to `out` and `err` before flushed first) and its exit code the
/// command's.
pub(super) struct RemoteCommand {
    pub(super) remote: &'static Remote,
    pub(super) name: String,
    pub(super) summary: String,
}

impl Command for RemoteCommand {
    fn name(&self) -> &str {
        &self.name
    }
    fn summary(&self) -> &str {
        &self.summary
    }
    fn run(&self, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
        use std::os::unix::process::ExitStatusExt as _;
        // The program writes to this process's streams itself, after what was written before.
        let _ = out.flush().and_then(|()| err.flush());
        let said = |err: &mut dyn Write, what: String| {
            let what = format!("its command {} {what}", self.name);
            let _ = writeln!(err, "{}", self.remote.failed(&what));
        };
        match run(&self.remote.process().command, &self.name, args) {
            Ok(status) => match (status.code(), status.signal()) {
                (Some(code), _) => code,
                (None, signal) => {
                    said(err, super::ended(status));
                    128 + signal.unwrap_or(0)
                }
            },
            Err(why) => {
                said(err, format!("cannot start: {why}"));
                1
            }
        }
    }
}

/// The plugin's `command` (its program and its arguments) run as `command NAME ARGS…`, as a linked
/// plugin's command runs: in this process's folder with its stdin, stdout and stderr, without the
/// session. How it ended.
fn run(command: &[String], name: &str, args: &[String]) -> io::Result<std::process::ExitStatus> {
    let (program, before) = command
        .split_first()
        .ok_or_else(|| io::Error::other("it names no program"))?;
    crate::children::command(program)
        .args(before)
        .arg("command")
        .arg(name)
        .args(args)
        .spawn()?
        .wait()
}
