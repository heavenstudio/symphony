//! Who bypasses the default branch's rules, verified again by each release run (docs/workflow/RELEASES.md,
//! "Who bypasses the rules, again"; docs/operations/GITHUB.md, "Who bypasses the rules"). The command
//! `[releases] rulesets` names, the GitHub adapter's `symphony-github rulesets --verify` where its
//! settings publish pull requests, is a check of the run of its own, [`NAME`], after the checks it
//! was given, and so recorded, red unless it exits 0, filed (`tests/release-red-rulesets`) and held
//! as a release candidate as any check is. It reads the rulesets with the gh CLI of whoever runs
//! the release, and keeps them where the policy holds: no package here builds with the adapter,
//! which is run as a program. While the project names no command, the run has no such check.
//!
//! It runs in the main checkout, as `[releases] github`'s command does, so a command named by a
//! relative path is the same program for both: the check's command changes to that folder first,
//! which its record and its red task say.

use std::path::Path;

use super::checks::Step;
use super::rerun::quoted;

/// The check's name: what its log, its record and its red task are named by.
pub(crate) const NAME: &str = "rulesets";

/// The check that runs `command` in the main checkout at `main`; none for no command.
pub(crate) fn step(main: &Path, command: &[String]) -> Option<Step> {
    if command.is_empty() {
        return None;
    }
    let words: Vec<String> = command.iter().map(|word| quoted(word)).collect();
    Some(Step::shell(
        NAME,
        &format!(
            "cd {} && {}",
            quoted(&main.to_string_lossy()),
            words.join(" ")
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::checks::{self, Within};
    use crate::release::waits::far;
    use crate::test_tree::Tree;
    use std::fs;

    fn words(command: &[&str]) -> Vec<String> {
        command.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn no_command_is_no_check_and_one_is_the_check_rulesets_in_the_main_checkout() {
        assert_eq!(step(Path::new("/main"), &[]), None);
        let step = step(
            Path::new("/projects/symphony"),
            &words(&["symphony-github", "rulesets", "--verify"]),
        )
        .unwrap();
        assert_eq!(
            step,
            Step::shell(
                "rulesets",
                "cd /projects/symphony && symphony-github rulesets --verify"
            )
        );
    }

    #[test]
    fn the_check_runs_the_command_whole_in_the_main_checkout_with_its_status() {
        let tree = Tree::new();
        let root = tree.root.canonicalize().unwrap();
        // A main checkout whose path a shell would split and expand, and a program in it named
        // by a relative path, which prints where it ran and each argument it was given.
        let main = root.join("main's $HOME checkout");
        fs::create_dir_all(main.join("bin")).unwrap();
        let program = main.join("bin/adapter");
        fs::write(
            &program,
            "#!/bin/sh\npwd\nfor word in \"$@\"; do echo \"[$word]\"; done\nexit \"$CODE\"\n",
        )
        .unwrap();
        fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let candidate = root.join("candidate");
        fs::create_dir_all(&candidate).unwrap();
        let within = Within::bound(far());
        let mut step = step(
            &main,
            &words(&["bin/adapter", "rulesets", "--verify", "a b", "*"]),
        )
        .unwrap();
        assert_eq!(
            step.command,
            format!(
                r"cd '{}/main'\''s $HOME checkout' && bin/adapter rulesets --verify 'a b' '*'",
                root.display()
            )
        );
        for (code, green) in [("0", true), ("1", false)] {
            step.set = vec![("CODE".into(), code.into())];
            let log = root.join(format!("rulesets-{code}.log"));
            let ran = checks::run(&step, &candidate, &log, within);
            assert_eq!(
                (ran.green(), ran.exit_code),
                (green, Some(code.parse().unwrap()))
            );
            assert_eq!(ran.name, "rulesets");
            assert_eq!(
                fs::read_to_string(&log).unwrap(),
                format!(
                    "$ {}\n{}\n[rulesets]\n[--verify]\n[a b]\n[*]\n",
                    step.command,
                    main.display()
                )
            );
        }
    }
}
