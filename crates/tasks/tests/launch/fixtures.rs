//! Immutable setup is built once; each World gets its own files, database, clock and processes.

use crate::common::{Fixture, Seed};
use crate::{Folder, World, git};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct File {
    path: PathBuf,
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}

/// The initial repository's files in memory, with no directory left behind to own or mutate.
fn repository_seed() -> &'static Vec<File> {
    static SEED: OnceLock<Vec<File>> = OnceLock::new();
    SEED.get_or_init(|| {
        let folder = Folder::new();
        git(&folder.0, &["init", "-q", "-b", "main", "--template="]);
        git(&folder.0, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let mut files = Vec::new();
        read_files(&folder.0, &folder.0, &mut files);
        symphony_board::order::sort_by(&mut files, &|a, b| a.path.cmp(&b.path));
        files
    })
}

fn read_files(root: &Path, dir: &Path, files: &mut Vec<File>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let metadata = fs::metadata(&path).unwrap();
        if metadata.is_dir() {
            read_files(root, &path, files);
        } else {
            files.push(File {
                path: path.strip_prefix(root).unwrap().to_path_buf(),
                bytes: fs::read(&path).unwrap(),
                permissions: metadata.permissions(),
            });
        }
    }
}

/// Materialize a full private repository, including private refs and objects: no shared hard links.
pub fn repository(repo: &Path) {
    for file in repository_seed() {
        let path = repo.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &file.bytes).unwrap();
        fs::set_permissions(path, file.permissions.clone()).unwrap();
    }
}

/// Preserve the public setup's rows and creation messages without repeating its writes per World.
pub fn board() -> (Fixture, String) {
    static SEED: OnceLock<(Seed, String)> = OnceLock::new();
    let (seed, orch) = SEED.get_or_init(|| {
        let f = Fixture::empty();
        let orch = f.session("orchestrator");
        f.task("sim/jump");
        let seed = Seed {
            dump: f.store.dump().unwrap(),
        };
        (seed, orch)
    });
    (Fixture::new(seed.clone()), orch.clone())
}

#[test]
fn seeded_worlds_keep_repository_changes_and_worktrees_private() {
    let a = World::new();
    let b = World::new();
    let original = git(&b.repo, &["rev-parse", "main"]);
    fs::write(a.repo.join("only-a"), "a").unwrap();
    git(&a.repo, &["add", "only-a"]);
    git(&a.repo, &["commit", "-q", "-m", "a"]);
    a.launch("sim/jump");
    assert_ne!(git(&a.repo, &["rev-parse", "main"]), original);
    assert_eq!(git(&b.repo, &["rev-parse", "main"]), original);
    assert!(!b.repo.join("only-a").exists());
    assert_eq!(git(&b.repo, &["branch", "--list"]), "* main");
    assert_eq!(
        git(&b.repo, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    // Materializing another repository after the mutation also starts from the immutable seed.
    let c = World::new();
    assert_eq!(git(&c.repo, &["rev-parse", "main"]), original);
    assert!(!c.repo.join("only-a").exists());
}

#[test]
fn seeded_worlds_keep_board_rows_clock_and_creation_history_private() {
    let a = World::new();
    let b = World::new();
    let initial = b.f.said("sim/jump");
    assert!(
        initial.iter().any(|line| line.contains("created by owner")),
        "{initial:?}"
    );
    let worker = a.f.session("worker");
    a.store().claim("sim/jump", &worker).unwrap();
    a.f.advance(17.0);
    assert_eq!(b.store().get("sim/jump").unwrap().status, "ready");
    assert_eq!(b.f.now(), crate::common::NOW);
    assert_eq!(b.f.said("sim/jump"), initial);
    let c = World::new();
    assert_eq!(c.store().get("sim/jump").unwrap().status, "ready");
    assert_eq!(c.f.said("sim/jump"), initial);
}
