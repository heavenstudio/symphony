//! The servers this board asked to end (`sf-tasks/ending/PID`), so that none is ever asked twice.
//! SurrealDB 3.3 takes a second signal for an immediate shutdown (it says so as it begins its
//! clean one: "A second signal will force an immediate shutdown"), before its memtable is written
//! to sstables, which leaves its log for the next start to replay, as a kill would. A server is
//! asked once ([`Home::end`]), by a file named by its pid, written before the signal: a later ask
//! of that pid while it runs, from this process or another, only waits, and a scratch board's
//! supervisor sends none of its own to a server that file names ([`super::OWNED_START`]). The file
//! goes once that server has ended: when an ask sees it end, when any ask finds its pid gone, and
//! when a start of this board's makes a new process of that pid ([`Home::started`]).

use super::{Home, LOG, ServerError, io};
use crate::sys::{local_minute, pid_alive, terminate};
use std::fs;
use std::time::{Duration, Instant};

/// In the board's folder: one file per server asked to end, named by its pid, holding when.
pub(super) const ENDING: &str = "ending";

impl Home {
    /// Ask `pid`, a server that answers, to end (SIGTERM), once, and wait while it closes: SurrealDB
    /// writes what its memtable holds to an sstable and empties its log before it ends. It is never
    /// made to end: a second signal ends it at once, before its store is closed, and a kill would
    /// leave its log for the next start to replay. A server asked before, by this process or
    /// another, is not asked again, only waited for. One still running after the closing patience
    /// is left to finish, and the stop said to be under way. Only a server that answers is asked:
    /// one still starting has no handler yet, and the signal would end it at once, as a kill does.
    /// An ask that cannot be written down first is not made, as nothing would then keep a later
    /// one from being a second.
    pub(super) fn end(&self, pid: i64) -> Result<(), ServerError> {
        self.forget_ended();
        let folder = self.dir.join(ENDING);
        let asked = folder.join(pid.to_string());
        // An ask that does not read is not taken for none: the server is then not asked.
        let before = match fs::read_to_string(&asked) {
            Ok(at) => Some(at.trim().parse::<f64>().unwrap_or_default()),
            Err(problem)
                if matches!(
                    problem.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                None
            }
            Err(problem) => return Err(io("could not read", &asked)(problem)),
        };
        if before.is_none() {
            fs::create_dir_all(&folder)
                .and_then(|()| fs::write(&asked, crate::store::now().to_string()))
                .map_err(|problem| {
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
            std::thread::sleep(Duration::from_millis(20));
        }
        let log = self.dir.join(LOG);
        Err(ServerError::NotUp(match before {
            None => format!(
                "the board's server (pid {pid}) is still closing its store {} s after it was asked \
                 to end: it is left to finish, never killed; its log is {}",
                self.closing.as_secs(),
                log.display()
            ),
            Some(at) => format!(
                "the board's server (pid {pid}) was asked to end at {} and is still closing its \
                 store: it is left to finish, never asked again (a second signal ends SurrealDB \
                 before its store is closed) nor killed; its log is {}",
                local_minute(at),
                log.display()
            ),
        }))
    }

    /// The asks of servers that have since ended forgotten. One that cannot be removed stays, which
    /// can only keep a later process of its pid from being asked.
    fn forget_ended(&self) {
        let Ok(entries) = fs::read_dir(self.dir.join(ENDING)) else {
            return;
        };
        for entry in entries.flatten() {
            let ended = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<i64>().ok())
                .is_some_and(|pid| !pid_alive(pid));
            if ended {
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
    use std::time::{Duration, Instant};

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

    /// Waits up to five seconds for `path` to be there.
    fn appears(path: &Path) {
        let began = Instant::now();
        while !path.exists() {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "{}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The SIGTERMs `dir`'s program took, once a moment has passed for its trap to run.
    fn terms(dir: &Path) -> usize {
        std::thread::sleep(Duration::from_millis(300));
        fs::read_to_string(dir.join("terms"))
            .unwrap_or_default()
            .lines()
            .count()
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
        assert!(first.contains("after it was asked to end"), "{first}");
        assert!(asked.exists(), "the ask is written down");
        assert_eq!(terms(&dir), 1);
        let again = home.end(pid).unwrap_err().to_string();
        assert!(
            again.starts_with(&format!(
                "the board's server (pid {pid}) was asked to end at "
            )) && again.contains("never asked again"),
            "{again}"
        );
        assert_eq!(terms(&dir), 1, "asked a second time");
        assert!(pid_alive(pid));

        server.kill().unwrap();
        server.wait().unwrap();
        home.end(pid).unwrap();
        assert!(!asked.exists(), "the ask outlived its server");
        assert_eq!(terms(&dir), 1);
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
        assert_eq!(terms(&dir), 0, "signalled without its ask written down");
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
                terms(&dir),
                usize::from(!already),
                "already asked: {already}"
            );
            assert!(pid_alive(pid));
            terminate(pid, true);
            let began = Instant::now();
            while pid_alive(pid) {
                assert!(
                    began.elapsed() < Duration::from_secs(5),
                    "{pid} survived its kill"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
