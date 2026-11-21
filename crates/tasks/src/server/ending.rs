//! The servers this board asked to end (`symphony/tasks/ending/PID`), so that none is ever asked
//! twice. SurrealDB 3.3 takes a second signal for an immediate shutdown (it says so as it begins
//! its clean one: "A second signal will force an immediate shutdown"), before its memtable is
//! written to sstables, which leaves its log for the next start to replay, as a kill would. A
//! server is asked once ([`Home::end`]), by a file named by its pid, written before the signal: a
//! later ask of that pid while it runs, from this process or another, only waits, and a scratch
//! board's supervisor sends none of its own to a server that file names ([`super::OWNED_START`]).
//! The file holds when the process it asked started ([`Home::process_start`]), and goes once that
//! server has ended: when an ask sees it end, when any ask finds its pid gone or held by a process
//! that started at another time (a pid since reused), and when a start of this board's makes a new
//! process of that pid ([`Home::started`]). So an ask left behind never keeps a later server of
//! that pid from being asked.

use super::{Home, LOG, ServerError, io};
use crate::sys::{local_minute, pid_alive, terminate};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// In the board's folder: one file per server asked to end, named by its pid ([`Ask`]).
pub(super) const ENDING: &str = "ending";

/// What a file of [`ENDING`] says: when its server was asked to end, and when its process started.
#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct Ask {
    /// When it was asked, in seconds since the epoch.
    at: f64,
    /// When the process asked started, as `ps` said it then; none where it could not, the ask then
    /// standing for whatever process has the pid while one does.
    #[serde(default)]
    started: Option<String>,
}

/// The ask at `path`: none when there is none; one that does not read as an ask stands for
/// whatever process has its pid, its start unknown; an error when it cannot be read.
fn read_ask(path: &Path) -> std::io::Result<Option<Ask>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(crate::objects::parse(&text).unwrap_or_default())),
        Err(problem)
            if matches!(
                problem.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(problem) => Err(problem),
    }
}

/// Whether `ask`, of `pid`, which runs, asked another process than the one that has the pid now:
/// one the kernel says started at another time. Not when the ask wrote down no start, or the
/// kernel does not say.
fn stale(pid: i64, ask: &Ask) -> bool {
    ask.started
        .as_ref()
        .is_some_and(|then| Home::process_start(pid).is_ok_and(|now| &now != then))
}

impl Home {
    /// Ask `pid`, a server that answers, to end (SIGTERM), once, and wait while it closes: SurrealDB
    /// writes what its memtable holds to an sstable and empties its log before it ends. It is never
    /// made to end: a second signal ends it at once, before its store is closed, and a kill would
    /// leave its log for the next start to replay. A server asked before, by this process or
    /// another, is not asked again, only waited for. One still running after the closing patience
    /// is left to finish, and the stop said to be under way. Only a server that answers is asked:
    /// one still starting has no handler yet, and the signal would end it at once, as a kill does.
    /// An ask that cannot be written down first is not made, as nothing would then keep a later
    /// one from being a second. One that stands for an earlier process of the pid is set aside
    /// first ([`Home::forget_ended`]). A server still closing after the patience is said with what
    /// the owner can do.
    pub(super) fn end(&self, pid: i64) -> Result<(), ServerError> {
        self.forget_ended();
        let folder = self.dir.join(ENDING);
        let asked = folder.join(pid.to_string());
        // An ask that does not read is not taken for none: the server is then not asked.
        let before = read_ask(&asked).map_err(io("could not read", &asked))?;
        if before.is_none() {
            let ask = Ask {
                at: crate::store::now(),
                started: Home::process_start(pid).ok(),
            };
            let next = folder.join(format!("{pid}.{}", std::process::id()));
            self.make(&folder)
                .map_err(|problem| std::io::Error::other(problem.to_string()))
                .and_then(|()| {
                    fs::write(
                        &next,
                        serde_json::to_string(&ask).expect("an ask serialises"),
                    )
                })
                .and_then(|()| fs::rename(&next, &asked))
                .map_err(|problem| {
                    let _ = fs::remove_file(&next);
                    ServerError::Io(format!(
                        "could not write {}: {problem}; the server (pid {pid}) is not asked to \
                         end, as nothing would then keep a later ask from being a second, which \
                         ends SurrealDB before it has closed its store",
                        asked.display()
                    ))
                })?;
            terminate(pid, false);
        }
        let began = Instant::now();
        while began.elapsed() < self.closing {
            if !pid_alive(pid) {
                let _ = fs::remove_file(&asked);
                return Ok(());
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: looks again whether a server it told to end has gone, which no wait reports for a process it did not start"
            )]
            std::thread::sleep(Duration::from_millis(20));
        }
        let asked_when = match &before {
            None => format!(
                "is still closing its store {} s after it was asked to end",
                self.closing.as_secs()
            ),
            Some(ask) => format!(
                "was asked to end at {} and is still closing its store",
                local_minute(ask.at)
            ),
        };
        Err(ServerError::NotUp(format!(
            "the board's server (pid {pid}) {asked_when}: it is left to finish, never asked again (a \
             second signal ends SurrealDB before its store is closed) nor killed. One writing a \
             large memtable takes minutes; one whose CPU time stands still across a minute (`ps \
             -o time= -p {pid}`) has hung. The owner then holds the board (`just tasks database \
             hold WHY --as HANDLE`, a hold that stands while it says the server still closes), \
             ends it by hand (`kill -9 {pid}`, its log then replayed at the next start) and \
             releases the hold; its ask, {}, is forgotten once it has ended (docs/TASKS.md, Stuck \
             servers). Its log is {}",
            asked.display(),
            self.dir.join(LOG).display()
        )))
    }

    /// The asks that no longer stand forgotten: of a pid no process has, or one a process has that
    /// started at another time than the one asked. One that cannot be removed stays, which can
    /// only keep a later process of its pid from being asked.
    fn forget_ended(&self) {
        let Ok(entries) = fs::read_dir(self.dir.join(ENDING)) else {
            return;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<i64>().ok())
            else {
                continue;
            };
            let gone = !pid_alive(pid)
                || read_ask(&entry.path()).is_ok_and(|ask| ask.is_some_and(|ask| stale(pid, &ask)));
            if gone {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// `pid`, a process a start of this board's has just made: an ask of an earlier process of that
    /// pid no longer holds. One that cannot be removed stays, which can only keep this one from
    /// being asked.
    pub(super) fn started(&self, pid: i64) {
        let _ = fs::remove_file(self.dir.join(ENDING).join(pid.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::ENDING;
    use crate::server::{Home, OWNED_START, ServerError};
    use crate::sys::{Stdio, command, pid_alive, terminate};
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// A folder of its own under the temporary one, for test `name`.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ending-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A program in `dir` that, once it is ready (the file `ready`), writes a line to `terms` for
    /// each SIGTERM it takes and goes on running: a server that is slow to close.
    fn closing_slowly(dir: &Path) -> PathBuf {
        let path = dir.join("closing-slowly");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\ntrap 'echo term >> \"{}\"' TERM\n: > \"{}\"\nwhile :; do sleep 0.05; done\n",
                dir.join("terms").display(),
                dir.join("ready").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The wall-clock backstop of these tests' waits, of the five seconds they once gave each: only
    /// a stall reaches it (RULES.md `tests/no-test-fails-on-load`).
    const PATIENT: Duration = test_time::backstop(Duration::from_secs(5));

    /// Waits for `path` to be there.
    #[track_caller]
    fn appears(path: &Path) {
        test_time::until_true(format!("{} is there", path.display()), PATIENT, || {
            path.exists()
        })
        .unwrap();
    }

    /// The SIGTERMs `dir`'s program took, once it has taken `least` and a moment more has passed
    /// for its trap to write another. The wait for `least` is on its condition, so a trap a loaded
    /// machine runs late is still counted. The moment after only lets a SIGTERM beyond it show; on
    /// a loaded machine it may be too short to, which can let a second ask go unseen but never
    /// fails a server asked right.
    #[track_caller]
    fn terms(dir: &Path, least: usize) -> usize {
        let taken = || {
            fs::read_to_string(dir.join("terms"))
                .unwrap_or_default()
                .lines()
                .count()
        };
        test_time::until_true(
            format!(
                "the program in {} has taken at least {least} SIGTERMs",
                dir.display()
            ),
            PATIENT,
            || taken() >= least,
        )
        .unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a test holds a moment to show no further SIGTERM comes, where a mark the program writes after the ask's last signal is the event"
        )]
        std::thread::sleep(Duration::from_millis(300));
        taken()
    }

    /// The pid of a process that has ended and been reaped.
    fn ended_pid() -> i64 {
        let mut child = command("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        i64::from(child.id())
    }

    /// A server is asked once: a second ask while it runs, this one still closing, only waits, and
    /// says when it was asked; once it has ended, the ask is forgotten.
    #[test]
    fn a_server_is_asked_to_end_once_and_never_again_while_it_runs() {
        let dir = folder("once");
        let program = closing_slowly(&dir);
        let mut server = command(&program)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = i64::from(server.id());
        appears(&dir.join("ready"));
        let home = Home::new(&dir, &dir.join("no-surreal")).closing(Duration::from_millis(200));
        let asked = dir.join(ENDING).join(pid.to_string());

        let first = home.end(pid).unwrap_err().to_string();
        assert!(
            first.starts_with(&format!(
                "the board's server (pid {pid}) is still closing its store 0 s after it was asked \
                 to end: it is left to finish, never asked again"
            )),
            "{first}"
        );
        for remedy in [
            format!("`ps -o time= -p {pid}`"),
            "`just tasks database hold WHY --as HANDLE`".to_string(),
            format!("`kill -9 {pid}`"),
            format!(
                "its ask, {}, is forgotten once it has ended",
                asked.display()
            ),
            "(docs/TASKS.md, Stuck servers)".to_string(),
        ] {
            assert!(first.contains(&remedy), "{remedy} in {first}");
        }
        let ask: super::Ask = serde_json::from_str(&fs::read_to_string(&asked).unwrap()).unwrap();
        assert_eq!(
            ask.started,
            Some(Home::process_start(pid).unwrap()),
            "the ask names the process it asked"
        );
        assert_eq!(terms(&dir, 1), 1);
        let again = home.end(pid).unwrap_err().to_string();
        assert!(
            again.starts_with(&format!(
                "the board's server (pid {pid}) was asked to end at "
            )) && again.contains("never asked again"),
            "{again}"
        );
        assert_eq!(terms(&dir, 1), 1, "asked a second time");
        assert!(pid_alive(pid));

        server.kill().unwrap();
        server.wait().unwrap();
        home.end(pid).unwrap();
        assert!(!asked.exists(), "the ask outlived its server");
        assert_eq!(terms(&dir, 1), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// An ask whose server has ended is forgotten by the next ask of any server, and one of a pid a
    /// start makes anew by that start; an ask of a process that runs stays.
    #[test]
    fn the_asks_of_ended_servers_and_of_pids_a_start_makes_anew_are_forgotten() {
        let dir = folder("forgotten");
        let home = Home::new(&dir, &dir.join("no-surreal"));
        let ending = dir.join(ENDING);
        fs::create_dir_all(&ending).unwrap();
        let (gone, runs) = (ended_pid(), i64::from(std::process::id()));
        for pid in [gone, runs] {
            fs::write(ending.join(pid.to_string()), "1794469500").unwrap();
        }
        fs::write(ending.join("notes"), "not a pid").unwrap();
        home.forget_ended();
        assert!(
            !ending.join(gone.to_string()).exists(),
            "an ended server's ask stays"
        );
        assert!(
            ending.join(runs.to_string()).exists(),
            "a running process's ask went"
        );
        assert!(ending.join("notes").exists());
        home.started(runs);
        assert!(!ending.join(runs.to_string()).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// An ask left behind for a process that has ended stands for no later one of its pid: one
    /// naming another start than the process that has the pid now is forgotten, so that process is
    /// asked as any other; one naming its own start stays.
    #[test]
    fn an_ask_naming_another_process_of_its_pid_is_set_aside() {
        let dir = folder("reused");
        let home = Home::new(&dir, &dir.join("no-surreal"));
        let ending = dir.join(ENDING);
        fs::create_dir_all(&ending).unwrap();
        let runs = i64::from(std::process::id());
        let asked = ending.join(runs.to_string());
        let ask = |started: &str| {
            serde_json::to_string(&super::Ask {
                at: 1_794_469_500.0,
                started: Some(started.to_string()),
            })
            .unwrap()
        };
        fs::write(&asked, ask(&Home::process_start(runs).unwrap())).unwrap();
        home.forget_ended();
        assert!(asked.exists(), "the ask of the process that runs went");
        fs::write(&asked, ask("1.000000")).unwrap();
        home.forget_ended();
        assert!(
            !asked.exists(),
            "an ask of an earlier process of the pid stays"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// When the kernel does not say when a process started, its ask is kept: a process of a
    /// reused pid may wait on it, never a server be asked twice.
    #[test]
    fn an_ask_is_kept_while_ps_cannot_say_when_its_process_started() {
        let dir = folder("untold-start");
        let home = Home::new(&dir, &dir.join("no-surreal"));
        let ending = dir.join(ENDING);
        fs::create_dir_all(&ending).unwrap();
        let asked = ending.join(std::process::id().to_string());
        fs::write(&asked, r#"{"at":1794469500.0,"started":"1.000000"}"#).unwrap();
        crate::server::PS_FAILS.set(u32::MAX);
        home.forget_ended();
        crate::server::PS_FAILS.set(0);
        assert!(asked.exists(), "an ask went on a `ps` that failed");
        let _ = fs::remove_dir_all(&dir);
    }

    /// An ask that cannot be written down first is not made: the server is not signalled, and the
    /// refusal says why.
    #[test]
    fn an_ask_that_cannot_be_written_down_is_not_made() {
        let dir = folder("unwritten");
        let program = closing_slowly(&dir);
        let mut server = command(&program)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = i64::from(server.id());
        appears(&dir.join("ready"));
        fs::write(dir.join(ENDING), "a file where the folder goes").unwrap();
        let home = Home::new(&dir, &dir.join("no-surreal")).closing(Duration::from_millis(50));
        let refused = home.end(pid).unwrap_err();
        assert!(
            matches!(&refused, ServerError::Io(why) if why.contains(&format!("the server (pid {pid}) is not asked to end"))),
            "{refused}"
        );
        assert_eq!(terms(&dir, 0), 0, "signalled without its ask written down");
        server.kill().unwrap();
        server.wait().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    /// The supervisor of a scratch board's server sends no signal of its own, once its starter's
    /// lifeline ends, to a server already asked to end; to one never asked, one SIGTERM.
    #[test]
    fn a_scratch_supervisor_never_signals_a_server_already_asked_to_end() {
        for already in [true, false] {
            let dir = folder(if already {
                "supervised-asked"
            } else {
                "supervised"
            });
            let program = closing_slowly(&dir);
            let ending = dir.join(ENDING);
            fs::create_dir_all(&ending).unwrap();
            let mut supervisor = command("/bin/sh")
                .arg("-c")
                .arg(OWNED_START)
                .arg(&program)
                .arg("127.0.0.1:1")
                .arg(format!("surrealkv://{}", dir.join("data").display()))
                .arg(dir.join("server.log"))
                .arg(&ending)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let lifeline = supervisor.stdin.take().unwrap();
            let mut said = String::new();
            supervisor
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut said)
                .unwrap();
            supervisor.wait().unwrap();
            let pid: i64 = said.trim().parse().unwrap();
            appears(&dir.join("ready"));
            if already {
                fs::write(ending.join(pid.to_string()), "1794469500").unwrap();
            }
            drop(lifeline);
            assert_eq!(
                terms(&dir, usize::from(!already)),
                usize::from(!already),
                "already asked: {already}"
            );
            assert!(pid_alive(pid));
            terminate(pid, true);
            test_time::until_true(format!("{pid} ended on its kill"), PATIENT, || {
                !pid_alive(pid)
            })
            .unwrap();
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
