//! The live board is known by its server, never by the caller's folder: a `TASKS_SERVER` or an
//! address from a folder outside any repository with an empty registry, and from a copy of the
//! repository that keeps a copy of the original's PostgreSQL data, reaches the original's board,
//! judged by the original's default branch.

use super::{COPY, Held, Live, OWN, TASKS, as_copy, before, refused};
use crate::children::Child as _;
use crate::{Ran, git};
use std::path::Path;
use symphony_board::live::SOURCE;

#[test]
fn an_address_reaches_the_board_its_server_serves_from_any_folder() {
    let live = Live::older("main-only-served", Held::Other);
    let why = live.another_source();
    let addr = live.addr();
    // A machine whose registry lists no project.
    let empty = live.base.join("empty-machine");
    std::fs::create_dir_all(&empty).unwrap();
    let from = |cwd: &Path, program: &str, args: &[&str], input: &str| -> Ran {
        let vars = [
            ("TASKS_SERVER", addr.as_str()),
            ("SYMPHONY_MACHINE_DIR", empty.to_str().unwrap()),
        ];
        live.run(Path::new(program), cwd, &vars, args, input)
    };

    // Outside any repository, nothing registered.
    refused(&from(&live.outside, TASKS, &["list"], ""), &why);
    let ran = from(&live.outside, COPY, &[&addr], "{}");
    assert_eq!((ran.code, ran.err), (2, as_copy(&why)));
    let main = live.main.display();
    refused(
        &from(&live.outside, TASKS, &["migrate"], ""),
        &format!(
            "`tasks migrate` moves the live board only from its main checkout, {main}: nothing was \
             read or written; run it there (`cd {main} && just tasks migrate`)"
        ),
    );

    // A copy of the repository, its board's data a copy of the original's, its default branch
    // committing this build's source: the board is the original's, judged by the original's
    // default branch.
    let copy = live.base.join("copy");
    let done = std::process::Command::of("cp")
        .arg("-R")
        .arg(&live.main)
        .arg(&copy)
        .status()
        .unwrap();
    assert!(done.success());
    assert!(
        copy.join(".git/symphony/tasks/postgresql/PG_VERSION")
            .is_file()
    );
    std::fs::write(copy.join(SOURCE), OWN).unwrap();
    git(&copy, &["commit", "-qam", "this build's source"]);
    refused(&from(&copy, TASKS, &["list"], ""), &why);
    let ran = from(&copy, COPY, &[&addr], "{}");
    assert_eq!((ran.code, ran.err), (2, as_copy(&why)));

    assert_eq!(live.version("tasks"), Some(before()));
}
