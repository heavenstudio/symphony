//! Run the real repository fixture in fresh owners, keeping them alive while checking cleanup.
//! A dead-owner sweep therefore cannot hide a missing successful Drop or remove a failed case early.

use std::os::unix::process::ExitStatusExt;

use super::owner::{OWNER, PREPARED_APP_BY_PARENT, prepare_images};
use super::*;

const ROLE: &str = "SYMPHONY_LIFECYCLE_SCRATCH_ROLE";
const ENTRY: &str = "fixture::scratch_tests::scratch_owner";

/// An owner makes its whole repository inside this readiness wait. Every new image that
/// repository runs is run here first, as setup ([`prepare_images`]), and the owner takes this
/// process's run of the app as its own: a freshly built app's first run waits its turn in the
/// machine's assessment queue, for as long as the queue holds it, which only setup waits for.
fn owner(role: &str, logs: &Scratch) -> (Process, PathBuf) {
    let images = prepare_images(&logs.path);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([ENTRY, "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
        .env(PREPARED_APP_BY_PARENT, images.app.to_string())
        .stdin(Stdio::piped());
    let mut process = Process::spawn(command, &logs.path.join(role));
    let id = process.child.id();
    let (stdout, stderr) = (process.stdout.clone(), process.stderr.clone());
    let launch = process.launch.clone();
    let child = &mut process.child;
    let path = poll(
        BACKSTOP,
        // The owner's output names each stage it finished, and when ([`owner_stage`]).
        || {
            format!(
                "scratch owner readiness for role {role}, child {id}; {launch}: {}",
                output(&stdout, &stderr)
            )
        },
        || {
            assert!(
                child.try_wait().unwrap().is_none(),
                "scratch owner exited before readiness: {}",
                output(&stdout, &stderr)
            );
            output(&stdout, &stderr)
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
                .find_map(|line| {
                    line.trim_end()
                        .split_once("SCRATCH_READY=")
                        .map(|(_, path)| path.into())
                })
        },
        Instant::elapsed,
    );
    let output = process.output();
    if symphony_env::PROFILE.os().is_some() {
        eprintln!("scratch owner {role} ready; {}: {output}", process.launch);
    }
    let used = output
        .lines()
        .find_map(|line| line.split_once("SCRATCH_APP=").map(|(_, pid)| pid));
    assert_eq!(
        used,
        Some(images.app.to_string().as_str()),
        "the owner's repository ran an app preparation of its own inside the readiness wait: {output}"
    );
    (process, path)
}

fn release(mut process: Process) {
    process.child.stdin.take().unwrap().write_all(b"x").unwrap();
    process.success();
    assert!(
        process.output().contains("test result: ok. 1 passed;"),
        "{}",
        process.output()
    );
}

fn unwind_with_descendant(repo: Repo, through_repo: bool, observer: &mut Option<Descendants>) -> ! {
    // Declare the writer before the moved repository: it stays open throughout both drop
    // paths, then releases the orphan when the original unwind ends.
    let _hold = fifo(&repo.base.join("descendant.hold"));
    let repository = repo;
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "exec 3< \"$1\"; /bin/sh -c 'printf ready; read -r release' <&3 & exit",
            "fixture",
        ])
        .arg(repository.base.join("descendant.hold"));
    let mut process = if through_repo {
        repository.run(command)
    } else {
        Process::spawn(command, &repository.log("descendant"))
    };
    process.success();
    until("unowned descendant readiness", || {
        (process.output() == "ready").then_some(())
    });
    println!("DESCENDANT_PID={}", process.child.id());
    let at_deadline = |descendant: &mut Descendants| {
        assert_eq!(Descendants::exited(&descendant.reader), Ok(false));
        let observer = Descendants {
            reader: descendant.reader.try_clone().unwrap(),
            origin: descendant.origin.clone(),
            wait_elapsed: Box::new(Instant::elapsed),
        };
        descendant.wait_elapsed = Box::new(|_| BACKSTOP);
        observer
    };
    *observer = Some(if through_repo {
        let mut descendants = repository.descendants.lock().unwrap();
        at_deadline(descendants.last_mut().unwrap())
    } else {
        at_deadline(process.descendants.as_mut().unwrap())
    });
    panic!("fixture retention failure");
}

/// Subprocess entry; ordinary discovery has no role and creates no scratch repository.
#[test]
fn scratch_owner() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    OWNER.set(Instant::now()).unwrap();
    owner_stage(&format!("{role} entered"));
    let occupied = (role == "collision").then(|| {
        // A reused pid must not adopt or overwrite a previous owner's directory.
        let occupied = Scratch {
            path: scratch_parent()
                .canonicalize()
                .unwrap()
                .join(format!("{SCRATCH_PREFIX}{}-0", std::process::id())),
            name: "previous owner".into(),
        };
        fs::create_dir(&occupied.path).unwrap();
        write(&occupied.path.join("keep"), "previous owner's files");
        occupied
    });
    let repo = Repo::new("scratch-owner");
    let path = repo.base.clone();
    if let Some(occupied) = occupied {
        assert_ne!(path, occupied.path);
        assert_eq!(
            fs::read_to_string(occupied.path.join("keep")).unwrap(),
            "previous owner's files"
        );
    }
    let held = match role.as_str() {
        "pass" | "collision" => {
            drop(repo);
            None
        }
        "fail" | "fail-process-descendant" | "fail-repo-descendant" => {
            let mut observer = None;
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Assertions after explicit process cleanup still need repository diagnostics.
                repo.cleanup();
                if role != "fail" {
                    unwind_with_descendant(repo, role == "fail-repo-descendant", &mut observer);
                }
                let _repo = repo;
                panic!("fixture retention failure");
            }));
            assert_eq!(
                failure.unwrap_err().downcast_ref::<&str>(),
                Some(&"fixture retention failure")
            );
            if let Some(mut observer) = observer {
                // The FIFO writer has dropped. Observe the orphan's exit before publishing the
                // retained scratch, which the parent checks while this owner remains alive.
                observer.wait();
            }
            None
        }
        "hold" => Some(repo),
        _ => panic!("unknown scratch owner role {role}"),
    };
    owner_stage("ready");
    println!("SCRATCH_APP={}", PREPARED_APP.get().unwrap());
    println!("SCRATCH_READY={}", path.display());
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    drop(held);
}

#[test]
fn passing_cases_remove_scratch_and_failed_cases_report_and_keep_it() {
    let logs = Scratch::new("scratch-retention-test");
    for role in ["pass", "collision"] {
        let (process, path) = owner(role, &logs);
        assert!(
            !path.exists(),
            "successful fixture left {} while its owner is still alive",
            path.display()
        );
        release(process);
    }

    for role in ["fail", "fail-process-descendant", "fail-repo-descendant"] {
        let (process, failed) = owner(role, &logs);
        assert!(failed.join("main with spaces/.git/HEAD").is_file());
        let output = process.output();
        assert!(output.contains("fixture retention failure"), "{output}");
        assert!(
            output.contains(&format!(
                "after_lifecycle case scratch-owner failed; scratch kept at {}",
                failed.display()
            )),
            "failure omitted its retained repository: {output}"
        );
        if role != "fail" {
            let pid: u32 = output
                .lines()
                .find_map(|line| line.split_once("DESCENDANT_PID=").map(|(_, pid)| pid))
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                output.contains(&format!(
                    "stalled: descendants of child {pid} (\"/bin/sh\" [\"-c\", "
                )),
                "unwind omitted its live descendant identity: {output}"
            );
            assert!(
                output.contains("to exit before scratch removal"),
                "{output}"
            );
        }
        // A normal test result proves cleanup preserved the caught original panic instead of
        // aborting this subprocess; the retained directory proves it did not erase diagnostics.
        release(process);

        let (next, _) = owner("pass", &logs);
        assert!(
            !failed.exists(),
            "next run did not sweep the dead failed owner"
        );
        release(next);
    }
}

#[test]
fn the_next_run_sweeps_a_killed_owner_and_preserves_a_live_owner() {
    let logs = Scratch::new("scratch-owner-test");
    let (mut held, path) = owner("hold", &logs);
    assert!(path.join("main with spaces/.git/HEAD").is_file());
    // Each command the held repository links runs a copy its owner's parent ran before the
    // owner started, or an image of the system's own under /usr/bin (tailscale's false).
    let copies = prepare_images(&logs.path).copies;
    for folder in [
        "commands",
        "main with spaces/tools",
        "prepare-commands/commands",
    ] {
        for entry in fs::read_dir(path.join(folder)).unwrap() {
            let link = entry.unwrap().path();
            let image = fs::read_link(&link).unwrap();
            assert!(
                copies.contains(&image) || image.starts_with("/usr/bin/"),
                "{} runs {}, which an owner's parent does not run first",
                link.display(),
                image.display()
            );
        }
    }
    let (next, _) = owner("pass", &logs);
    assert!(
        path.join("main with spaces/.git/HEAD").is_file(),
        "next run swept a live owner's repository"
    );
    release(next);

    held.signal(libc::SIGKILL);
    assert_eq!(held.wait().signal(), Some(libc::SIGKILL));
    let (next, _) = owner("pass", &logs);
    assert!(
        !path.exists(),
        "next run left killed owner's repository: {path:?}"
    );
    release(next);
}

/// The ps stand-in asks /bin/ps of each process alone and prints what one call over them all
/// prints. Given two pids, even one twice, /bin/ps reads the whole process table: seconds under
/// load, which each owner's command preparation spent inside a case's waits.
#[test]
fn the_ps_stand_in_asks_ps_of_each_process_alone_and_prints_one_calls_rows() {
    let scratch = Scratch::new("ps-stand-in-pids");
    let _stand_ins = StandIns::open(&scratch.path);
    let ps = scratch.path.join("commands/ps");
    fs::create_dir(ps.parent().unwrap()).unwrap();
    command_fixture(&ps, "ps.sh");
    // Beside this process, which the run's own env record names again: launchd, which outlives
    // every case; a child in a session of its own, away from this process's terminal if it has
    // one, which ps sorts before it; and a child already reaped, which has no row.
    let mut live = Command::new("/bin/cat");
    live.new_session().stdin(Stdio::piped());
    let mut live = Process::spawn(live, &scratch.path.join("live"));
    let mut dead = Process::spawn(Command::new("/usr/bin/true"), &scratch.path.join("dead"));
    dead.success();
    let named = [1, live.child.id(), dead.child.id()];
    for pid in named {
        write(&scratch.path.join(format!("owned.{pid}")), "");
    }
    let pids = [std::process::id()].into_iter().chain(named);
    let mut each: Vec<String> = pids.clone().map(|pid| pid.to_string()).collect();
    symphony_board::order::sort_by(&mut each, &Ord::cmp);
    let all = pids
        .map(|pid| pid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    // Fields with no title, and the same with the header ps prints for them.
    for fields in ["pid=,ppid=,command=", "pid,ppid,command"] {
        let mut command = Command::new("/bin/bash");
        command
            .arg("-x")
            .arg(&ps)
            .args(["-axww", "-o", fields])
            .env_clear()
            .env("HOME", &scratch.path)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null());
        let mut run = Process::spawn(command, &scratch.path.join(format!("run {fields}")));
        run.success();
        let traced = fs::read_to_string(&run.stderr).unwrap();
        let mut asked: Vec<&str> = traced
            .lines()
            .filter_map(|line| {
                line.trim_start_matches('+')
                    .strip_prefix(" /bin/ps -ww -p ")
            })
            .map(|rest| rest.split_once(' ').unwrap().0)
            .collect();
        symphony_board::order::sort_by(&mut asked, &Ord::cmp);
        assert_eq!(asked, each, "one /bin/ps for each process: {traced}");
        let one = symphony_board::sys::output(
            Command::new("/bin/ps").args(["-ww", "-p", &all, "-o", fields]),
        )
        .unwrap();
        assert!(one.status.success(), "{}", said(&one));
        assert_eq!(
            fs::read_to_string(&run.stdout).unwrap(),
            String::from_utf8(one.stdout).unwrap(),
            "the rows one /bin/ps over {all} prints: {traced}"
        );
    }
    drop(live.child.stdin.take());
    live.success();
}
