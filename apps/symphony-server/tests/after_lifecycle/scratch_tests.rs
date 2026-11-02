//! Run the real repository fixture in fresh owners, keeping them alive while checking cleanup.
//! A dead-owner sweep therefore cannot hide a missing successful Drop or remove a failed case early.

use std::os::unix::process::ExitStatusExt;

use super::*;

const ROLE: &str = "SF_LIFECYCLE_SCRATCH_ROLE";
const ENTRY: &str = "fixture::scratch_tests::scratch_owner";

fn owner(role: &str, logs: &Scratch) -> (Process, PathBuf) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([ENTRY, "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
        .stdin(Stdio::piped());
    let mut process = Process::spawn(command, &logs.path.join(role));
    let path = until("scratch owner readiness", || {
        assert!(
            process.child.try_wait().unwrap().is_none(),
            "scratch owner exited before readiness: {}",
            process.output()
        );
        process
            .output()
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .find_map(|line| {
                line.trim_end()
                    .split_once("SCRATCH_READY=")
                    .map(|(_, path)| path.into())
            })
    });
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
        descendant.wait_elapsed = Box::new(|_| PATIENCE);
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
    let occupied = (role == "collision").then(|| {
        // A reused pid must not adopt or overwrite a previous owner's directory.
        let occupied = Scratch {
            path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../artifacts/after_lifecycle")
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
                    "timed out: descendants of child {pid} (\"/bin/sh\" [\"-c\", "
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
