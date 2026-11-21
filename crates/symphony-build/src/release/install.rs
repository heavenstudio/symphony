//! A green release's install on the machine (docs/RELEASES.md, "The install"), the first step of
//! its refresh, `refresh-install`: the release built once, for every project, and put in the
//! machine folder, where the recipes of a project with no build of its own find it once `current`
//! names it.
//!
//! In the run's own checkout, at the release's commit and in its build folder, it builds the
//! release profile of the four binaries a project runs (`symphony-server`, `tasks`, `agent-send`,
//! `symphony`), the server with no client compiled in (`SYMPHONY_CLIENT_BESIDE`): it serves the one
//! installed beside it. The client is the distribution the commit holds
//! (`apps/symphony-server/client/public/`), which the release's own verification compared byte for
//! byte with a build of its sources, so nothing builds it again. What the release serves is asked
//! of the binaries that build made, never of this program, which a retry runs from a later main: the
//! server its plugin protocol and board schema (`symphony-server --serves`), the board its schema
//! and its `tasks api` version (`tasks api`, the method `version`).
//!
//! Then it installs them as `releases/NAME/` in the machine folder, whole
//! ([`symphony::machine::release::install_noted`]), with what a project changes to take it and
//! each release before it (`notes.md`, [`super::consumer::document_at`]). A build or an install that fails leaves no folder and
//! `current` where it was, and is the refresh's red, filed on the Symphony's board; `just release
//! --refresh` is the retry. A release already installed whole from the same commit is not built
//! again. The install points nothing: the refresh points `current` at the release ([`point`]) only
//! once nothing holds the switch and every board it moves has moved (`super::refresh`), a failure
//! to point being this step's red. Nothing here starts, stops or signals a server.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::checks::{self, Ran, Step, Within};
use super::group::{Bound, Failed, output_until};
use symphony::children::command;
use symphony::machine::release::{self, Built, Manifest, Serves, Version};

/// The step's name: one of the refresh's own, so its red is a refresh's red.
pub(crate) const STEP: &str = "refresh-install";
/// A program run in place of `bash tools/cargo.sh` for the release's build, with its arguments and
/// environment: the tests' stand-in for cargo. No flag sets it.
pub(crate) const BUILDER: &str = "SYMPHONY_RELEASE_BUILDER";
/// The packages that hold the release's binaries ([`release::BINARIES`]): the server's, the
/// board's (`tasks` and `agent-send`) and the library's (`symphony`).
const PACKAGES: [&str; 3] = ["symphony-server", "symphony-board", "symphony-app"];
/// The client's distribution, in the release's checkout.
const CLIENT: &str = "apps/symphony-server/client/public";
/// What tells the server's build its client is beside it (`symphony_env::CLIENT_BESIDE`).
const BESIDE: &str = "SYMPHONY_CLIENT_BESIDE";
/// The variables the build and the binaries it asks never keep: the board, session and task a run
/// names, and a toolchain of the starter's.
const UNSET: [&str; 7] = [
    "TASKS_SERVER",
    "TASKS_DATABASE",
    "TASKS_SESSION",
    "TASKS_TASK",
    "TASKS_RUN_PARENT",
    "TASKS_WORK",
    "RUSTUP_TOOLCHAIN",
];

/// What an install runs against: the run's checkout at the release's commit, the machine folder,
/// the release and its commit, where its log goes and its time.
pub(crate) struct Install<'a> {
    pub(crate) checkout: &'a Path,
    pub(crate) machine: &'a Path,
    pub(crate) tag: &'a str,
    pub(crate) commit: &'a str,
    pub(crate) logs: &'a Path,
    /// What [`BUILDER`] names, when it names one.
    pub(crate) builder: Option<&'a std::ffi::OsStr>,
    pub(crate) until: Bound<'a>,
}

/// An install's end: its step, green or red, what it said, and the manifest of the release it
/// installed or found installed already, none when it is red.
pub(crate) struct Installed {
    pub(crate) ran: Ran,
    pub(crate) said: Vec<String>,
    pub(crate) manifest: Option<Manifest>,
}

/// The step as the record and a red task say it.
pub(crate) fn step(tag: &str) -> Step {
    Step::shell(STEP, &format!("the install of {tag} on the machine"))
}

/// The install's log, among the refresh's.
pub(crate) fn log(logs: &Path) -> PathBuf {
    logs.join(format!("{STEP}.log"))
}

/// The release of `install` built and installed in the machine folder, `current` as it was.
pub(crate) fn install(install: &Install<'_>) -> Installed {
    let log = log(install.logs);
    let _ = fs::create_dir_all(install.logs);
    let named = step(install.tag);
    // A retry: the release is there whole. Never built again, never written into.
    match release::installed(install.machine, install.tag) {
        Ok(Some(manifest)) if manifest.commit == install.commit => {
            let said = vec![format!(
                "{} is installed already in {}: {}",
                install.tag,
                release::folder(install.machine, install.tag).display(),
                manifest.sizes()
            )];
            // The step first: it starts the log, which the line then follows.
            let ran = done(&named, &log, 0.0);
            append(&log, &said[0]);
            return Installed {
                ran,
                said,
                manifest: Some(manifest),
            };
        }
        Ok(Some(manifest)) => {
            let why = format!(
                "{} holds the commit {}, not the release's {}: an installed release is never \
                 written into, so remove that folder by hand if it is not the release",
                release::folder(install.machine, install.tag).display(),
                manifest.commit,
                install.commit
            );
            return never(&named, &log, why);
        }
        Err(problem) => {
            let why = format!(
                "the release folder is there and is not a whole release ({problem}): remove it by \
                 hand, and the next refresh installs it"
            );
            return never(&named, &log, why);
        }
        Ok(None) => {}
    }
    let target = install.checkout.join(".target");
    let mut ran = checks::run(
        &build(&named, &target, install.builder),
        install.checkout,
        &log,
        Within {
            bound: install.until,
            running: None,
        },
    );
    if !ran.green() {
        let said = vec![format!(
            "{} is not installed: its build failed ({}), and nothing of it is in {}",
            install.tag,
            ran.ended(),
            install.machine.join(release::RELEASES).display()
        )];
        return Installed {
            ran,
            said,
            manifest: None,
        };
    }
    let binaries = target.join("release");
    // What a project changes to take it, and each release before it, from the checkout's history:
    // a checkout git cannot read installs the release without them, which its log says.
    let notes = match super::consumer::document_at(install.checkout, install.tag, install.commit) {
        Ok(notes) => Some(notes),
        Err(why) => {
            append(&log, &format!("no {}: {why}", release::NOTES));
            None
        }
    };
    let made = serves(&binaries, install).and_then(|serves| {
        release::install_noted(
            install.machine,
            &Built {
                release: install.tag,
                commit: install.commit,
                serves,
                binaries: &binaries,
                client: &install.checkout.join(CLIENT),
            },
            notes.as_deref(),
        )
    });
    let manifest = match made {
        Ok(manifest) => manifest,
        Err(why) => {
            fail(&mut ran, &log, &why);
            let said = vec![format!(
                "{} is not installed: {why}; nothing of it is in {}",
                install.tag,
                install.machine.join(release::RELEASES).display()
            )];
            return Installed {
                ran,
                said,
                manifest: None,
            };
        }
    };
    let said = vec![
        format!(
            "installed {} in {}, built in {:.0} s: {}",
            install.tag,
            release::folder(install.machine, install.tag).display(),
            ran.seconds,
            manifest.sizes()
        ),
        served(&manifest),
    ];
    append(&log, &said.join("\n"));
    Installed {
        ran,
        said,
        manifest: Some(manifest),
    }
}

/// What `manifest`'s release serves, in one line.
fn served(manifest: &Manifest) -> String {
    let Serves {
        schema,
        additions,
        protocol,
        tasks_api,
        pin,
    } = manifest.serves;
    let steps = match additions {
        0 => String::new(),
        1 => " with its additive step".to_string(),
        steps => format!(" with its {steps} additive steps"),
    };
    format!(
        "{} serves board schema {schema}{steps}, plugin protocol {}.{} and tasks api {}.{}, at \
         Symphony pin {pin}",
        manifest.release, protocol.major, protocol.minor, tasks_api.major, tasks_api.minor
    )
}

/// Point `machine`'s `current` at the release `installed` holds, and say so among its lines:
/// whether it did. A failure turns its step red, `current` as it was; a red install points nothing.
pub(crate) fn point(machine: &Path, installed: &mut Installed) -> bool {
    let Some(tag) = (installed.manifest.as_ref()).map(|manifest| manifest.release.clone()) else {
        return false;
    };
    let pointed = match release::point(machine, &tag) {
        Ok(before) => {
            let was = match before.as_deref() {
                Some(before) if before == tag => " (as it did)".to_string(),
                Some(before) => format!(" (it named {before}, which stays installed)"),
                None => " (it named no release)".to_string(),
            };
            installed.said.push(format!(
                "{} names {tag}{was}: a project with no build of its own runs it",
                machine.join(release::CURRENT).display(),
            ));
            true
        }
        Err(why) => {
            let log = installed.ran.log.clone();
            fail(&mut installed.ran, &log, &why);
            (installed.said).push(format!(
                "{tag} is installed, and current is as it was: {why}"
            ));
            false
        }
    };
    let last = installed.said.last().map_or("", String::as_str);
    append(&installed.ran.log, last);
    pointed
}

/// The build of the release's binaries as `named`, into `target`, by `builder` in cargo's place
/// when one is named.
fn build(named: &Step, target: &Path, builder: Option<&std::ffi::OsStr>) -> Step {
    let mut argv: Vec<String> = match builder {
        Some(builder) => vec![builder.to_string_lossy().into_owned()],
        None => vec!["bash".into(), "tools/cargo.sh".into()],
    };
    argv.extend(["build", "--release", "--locked"].map(String::from));
    for package in PACKAGES {
        argv.extend(["-p".to_string(), package.to_string()]);
    }
    for binary in release::BINARIES {
        argv.extend(["--bin".to_string(), binary.to_string()]);
    }
    Step {
        argv,
        set: vec![
            (BESIDE.into(), "1".into()),
            (
                "CARGO_TARGET_DIR".into(),
                target.to_string_lossy().into_owned(),
            ),
        ],
        ..named.clone()
    }
}

/// What the binaries built in `binaries` say they serve, each asked within the install's time.
fn serves(binaries: &Path, install: &Install<'_>) -> Result<Serves, String> {
    let server = binaries.join("symphony-server");
    let said = asked(command(&server).arg("--serves"), &server, install)?;
    let tasks = binaries.join("tasks");
    let request = install.logs.join(format!("{STEP}.request.json"));
    fs::write(&request, "{\"method\": \"version\"}\n")
        .map_err(|error| format!("{}: {error}", request.display()))?;
    // A bounded command's stdin is null: the shell gives the board its one call from the file.
    let answered = asked(
        command("sh")
            .args(["-c", "exec \"$0\" api < \"$1\""])
            .arg(&tasks)
            .arg(&request),
        &tasks,
        install,
    );
    let _ = fs::remove_file(&request);
    read_serves(&said, &answered?)
}

/// [`serves`] read from the server's line `server` and the board's answer `board`.
fn read_serves(server: &str, board: &str) -> Result<Serves, String> {
    let version = |value: &Value, what: &str| -> Result<Version, String> {
        let part = |name: &str| {
            (value.get(name).and_then(Value::as_u64))
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| format!("the release's build names no {what} {name}"))
        };
        Ok(Version {
            major: part("major")?,
            minor: part("minor")?,
        })
    };
    let schema = |value: &Value, whose: &str| {
        (value.get("schema").and_then(Value::as_i64))
            .ok_or_else(|| format!("the release's {whose} names no board schema"))
    };
    let said: Value = serde_json::from_str(server.trim())
        .map_err(|error| format!("what symphony-server --serves said is not JSON: {error}"))?;
    let answer: Value = serde_json::from_str(board.trim())
        .map_err(|error| format!("what tasks api answered is not JSON: {error}"))?;
    let Some(board) = answer.get("ok") else {
        return Err(format!(
            "tasks api did not answer its version: {}",
            board.trim()
        ));
    };
    let (ours, theirs) = (schema(&said, "symphony-server")?, schema(board, "tasks")?);
    if ours != theirs {
        return Err(format!(
            "the release's symphony-server is built at board schema {ours} and its tasks at \
             {theirs}: one release holds one schema"
        ));
    }
    // A server built before it said its pin says none: 0, older than every project's.
    let pin = (said.get("pin").and_then(Value::as_u64)).map_or(Ok(0), |pin| {
        u32::try_from(pin).map_err(|_| format!("the release's symphony-server names the pin {pin}"))
    })?;
    // And one built before the contract, its additive steps: none, as it had none.
    let additions = match said.get("additions") {
        None => 0,
        Some(steps) => (steps.as_i64().filter(|steps| *steps >= 0)).ok_or_else(|| {
            format!("the release's symphony-server names the additive steps {steps}")
        })?,
    };
    Ok(Serves {
        schema: ours,
        additions,
        protocol: version(&said["protocol"], "plugin protocol")?,
        tasks_api: version(&board["api"], "tasks api")?,
        pin,
    })
}

/// What `asking`, the binary `binary` run with what it is asked, prints, within the install's time.
fn asked(
    asking: &mut symphony_board::sys::Command,
    binary: &Path,
    install: &Install<'_>,
) -> Result<String, String> {
    for name in UNSET {
        asking.env_remove(name);
    }
    asking.current_dir(install.checkout);
    let name = binary.file_name().unwrap_or_default().to_string_lossy();
    match output_until(asking, install.until) {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(format!(
            "the release's {name}, asked what it serves, ended {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(Failed::Io(error)) => Err(format!("the release's {name} did not run: {error}")),
        Err(Failed::Late(_)) => Err(format!(
            "the refresh's time was up while the release's {name} was asked what it serves"
        )),
        Err(Failed::Interrupted(_)) => Err(format!(
            "the run was told to stop while the release's {name} was asked what it serves"
        )),
    }
}

/// `named` green without a command of its own, after `seconds`.
fn done(named: &Step, log: &Path, seconds: f64) -> Ran {
    Ran {
        exit_code: Some(0),
        error: None,
        seconds,
        ..Ran::never(named, log, String::new())
    }
}

/// `named` red before anything was built, for `why`, said too.
pub(crate) fn never(named: &Step, log: &Path, why: String) -> Installed {
    Installed {
        said: vec![format!("not installed: {why}")],
        ran: Ran::never(named, log, why),
        manifest: None,
    }
}

/// Turn `ran` red for `why`, which its log ends with.
fn fail(ran: &mut Ran, log: &Path, why: &str) {
    ran.error = Some(why.to_string());
    append(log, why);
}

/// `line` added to the log at `log`.
fn append(log: &Path, line: &str) {
    let _ = fs::OpenOptions::new()
        .append(true)
        .open(log)
        .and_then(|mut file| writeln!(file, "{line}"));
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
