//! This repository's installed skills (`.agents/skills/NAME/`, what its own agents read) are what
//! `symphony init` writes from the shipped source at the same commit
//! (`crates/symphony/shipped/skills/NAME/`), text and stamp. The judgment is init's own: it runs
//! over a copy of the installed skills in a scratch project, and a skill it would write anything
//! into is behind its source or edited by hand. A `project.md` beside a role's skill is the
//! project's own, which init never reads or writes, so it is passed over.
//!
//! A land's merge meets this at the stamp, the digest of every file of a skill: two branches that
//! each change the same shipped skill and run init differ on its one stamp line, so git conflicts
//! there and the merge is settled by running init again, never by taking a side by hand, which this
//! test refuses.

use crate::common::TempDir;
use std::path::{Path, PathBuf};
use symphony::init::{Outcome, Shipped, Skill, init};

/// Where init installs a project's skills.
const SKILLS: &str = ".agents/skills";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy(&source, &target);
        } else {
            std::fs::copy(&source, &target).unwrap();
        }
    }
}

/// One line for each skill installed under `root` that init would write from `shipped`, naming it
/// and what to run; none when the installed skills are what init writes.
fn stale(root: &Path, shipped: &Shipped) -> Vec<String> {
    let scratch = TempDir::new("installed-skills");
    if root.join(SKILLS).is_dir() {
        copy(&root.join(SKILLS), &scratch.join(SKILLS));
    }
    init(&scratch, shipped)
        .unwrap()
        .into_iter()
        .filter_map(|(entry, outcome)| {
            let name = entry.strip_prefix("skill ")?;
            let how = match outcome {
                Outcome::Current => return None,
                Outcome::Updated => "is behind its shipped source".to_string(),
                Outcome::Replaced(_) => {
                    "differs from its stamp: it was edited where it is installed, or its stamp \
                     line was written by hand"
                        .to_string()
                }
                Outcome::Installed => "is not installed".to_string(),
                Outcome::Own => "carries no stamp, so init never writes it".to_string(),
                Outcome::Removed => "is installed and no longer shipped".to_string(),
            };
            Some(format!(
                "{SKILLS}/{name} {how} (crates/symphony/shipped/skills/{name}): run \
                 `tools/rs-bin.sh symphony init --root .` and commit what it writes"
            ))
        })
        .collect()
}

#[test]
fn this_repositorys_installed_skills_are_what_init_writes_from_the_shipped_source() {
    let shipped = Shipped::builtin();
    assert!(shipped.skills.iter().any(|skill| skill.name == "tasks"));
    let stale = stale(&workspace(), &shipped);
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

/// A project init just wrote the built-in skills into, removed when dropped.
fn installed() -> TempDir {
    let root = TempDir::new("installed-skills-project");
    init(&root, &Shipped::builtin()).unwrap();
    root
}

fn edit(root: &Path, file: &str, change: impl Fn(String) -> String) {
    let path = root.join(SKILLS).join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(path, change(text)).unwrap();
}

/// The built-in skills with `change` made to one file of one of them: a shipped source edited
/// since init last ran.
fn shipped_with(skill: &str, file: &str, change: &str) -> Shipped {
    let mut shipped = Shipped::builtin();
    let files = &mut shipped
        .skills
        .iter_mut()
        .find(|shipped| shipped.name == skill)
        .unwrap()
        .files;
    files.iter_mut().find(|(path, _)| path == file).unwrap().1 += change;
    shipped
}

fn one(stale: &[String]) -> &str {
    let [line] = stale else {
        panic!("one skill is named: {stale:?}");
    };
    assert!(
        line.contains("`tools/rs-bin.sh symphony init --root .`"),
        "{line}"
    );
    line
}

#[test]
fn what_init_just_wrote_is_current_and_a_project_md_beside_a_skill_is_passed_over() {
    let root = installed();
    assert_eq!(stale(&root, &Shipped::builtin()), [""; 0]);
    edit(&root, "work/project.md", |_| "The project's own.\n".into());
    edit(&root, "tasks/project.md", |_| "Its own too.\n".into());
    assert_eq!(stale(&root, &Shipped::builtin()), [""; 0]);
    // The check wrote nothing where the skills are installed.
    assert_eq!(
        std::fs::read_to_string(root.join(SKILLS).join("work/project.md")).unwrap(),
        "The project's own.\n"
    );
}

#[test]
fn a_shipped_file_edited_without_init_names_its_skill_alone() {
    let root = installed();
    // The skill's page and a file beside it, each behind its source.
    let line = stale(
        &root,
        &shipped_with("tasks", "SKILL.md", "One more line.\n"),
    );
    assert!(
        one(&line).starts_with(".agents/skills/tasks is behind its shipped source"),
        "{line:?}"
    );
    let line = stale(&root, &shipped_with("work", "build.md", "One more line.\n"));
    assert!(
        one(&line).starts_with(".agents/skills/work is behind its shipped source"),
        "{line:?}"
    );
    // A file the source gained since.
    let mut shipped = Shipped::builtin();
    shipped.skills[1]
        .files
        .push(("more.md".into(), "More.\n".into()));
    let name = shipped.skills[1].name.clone();
    let line = stale(&root, &shipped);
    assert!(
        one(&line).starts_with(&format!(".agents/skills/{name} is behind")),
        "{line:?}"
    );
}

#[test]
fn an_installed_copy_edited_by_hand_or_stamped_by_hand_names_its_skill() {
    let shipped = Shipped::builtin();
    // A file the stamp names, edited where it is installed.
    let root = installed();
    edit(&root, "work/start.md", |text| text + "An edit.\n");
    let line = stale(&root, &shipped);
    assert!(
        one(&line).starts_with(".agents/skills/work differs from its stamp"),
        "{line:?}"
    );
    // One side of a merge's stamp taken by hand: the text is the source's, the stamp another's.
    let root = installed();
    edit(&root, "tasks/SKILL.md", |text| {
        let at = text.find("symphony-stamp: ").unwrap() + "symphony-stamp: ".len();
        format!("{}0000000000000000{}", &text[..at], &text[at + 16..])
    });
    let line = stale(&root, &shipped);
    assert!(
        one(&line).starts_with(".agents/skills/tasks differs from its stamp"),
        "{line:?}"
    );
    // A stamp line left in conflict.
    let root = installed();
    edit(&root, "tasks/SKILL.md", |text| {
        let stamp = text
            .lines()
            .find(|line| line.contains("symphony-stamp: "))
            .unwrap();
        text.replacen(
            stamp,
            &format!("<<<<<<< HEAD\n{stamp}\n=======\n{stamp}\n>>>>>>> main"),
            1,
        )
    });
    let line = stale(&root, &shipped);
    assert!(
        one(&line).starts_with(".agents/skills/tasks differs from its stamp"),
        "{line:?}"
    );
    // A stamped file gone.
    let root = installed();
    std::fs::remove_file(root.join(SKILLS).join("work/finish.md")).unwrap();
    let line = stale(&root, &shipped);
    assert!(
        one(&line).starts_with(".agents/skills/work differs from its stamp"),
        "{line:?}"
    );
}

#[test]
fn a_skill_missing_unstamped_or_no_longer_shipped_is_named() {
    let root = installed();
    std::fs::remove_dir_all(root.join(SKILLS).join("triage")).unwrap();
    let line = stale(&root, &Shipped::builtin());
    assert!(
        one(&line).starts_with(".agents/skills/triage is not installed"),
        "{line:?}"
    );
    // Nothing installed at all names every skill.
    let empty = TempDir::new("installed-skills-none");
    assert_eq!(
        stale(&empty, &Shipped::builtin()).len(),
        Shipped::builtin().skills.len()
    );

    let root = installed();
    edit(&root, "chat-doc/SKILL.md", |text| {
        text.lines()
            .filter(|line| !line.contains("symphony-stamp: "))
            .map(|line| format!("{line}\n"))
            .collect()
    });
    let line = stale(&root, &Shipped::builtin());
    assert!(
        one(&line).starts_with(".agents/skills/chat-doc carries no stamp"),
        "{line:?}"
    );

    let root = installed();
    let mut shipped = Shipped::builtin();
    shipped.skills.push(Skill {
        name: "gone".into(),
        files: vec![("SKILL.md".into(), "---\nname: gone\n---\n\nGone.\n".into())],
    });
    init(&root, &shipped).unwrap();
    let line = stale(&root, &Shipped::builtin());
    assert!(
        one(&line).starts_with(".agents/skills/gone is installed and no longer shipped"),
        "{line:?}"
    );
}
