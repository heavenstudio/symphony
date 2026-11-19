//! The receipts of the lands a pull request delivered (docs/TASKS.md, "A land delivered by a pull
//! request"), as the release run reads them: `receipts/SLUG.json` in the lands' folder of the git
//! common dir, each written by `tasks land` once it proved the commit the host made
//! ([`symphony_board::land::delivery::Receipt`]). A squash leaves its task's head in no history
//! of the default branch, so the receipt is the one thing that says a commit there is a landing,
//! of which task, and which head it delivered: the cadence counts by it ([`super::cadence`]), a
//! release candidate advances by it ([`super::candidate`]), and a commit a squash delivered is
//! held by the line through it ([`Receipts::holds`]).
//!
//! A receipt is never taken at its word alone. One counts only for a commit whose one parent and
//! whose tree are the ones it recorded ([`borne_out`]), read from git; a commit's subject, its
//! patch or a pull request's title says nothing here. This module reads: it writes no receipt and
//! no reference, and asks no host anything.

use std::fs;
use std::path::{Path, PathBuf};

use symphony_board::land::delivery::{RECEIPTS, RETAINED, Receipt};

use super::git::{git, is_ancestor, short};

/// The lands' folder of the repository at `root`, in its git common dir; none where `root` is in
/// no repository.
pub(crate) fn lands(root: &Path) -> Option<PathBuf> {
    let common = symphony_board::git::common_dir(root).ok()?;
    Some(symphony_board::sweep::trigger::folder(&common).join(symphony_board::land::FOLDER))
}

/// Whether the commit whose parents are `parents` (git's `%P`) and whose tree is `tree` is the
/// one `receipt` recorded: of one parent, the base the squash was made on, and of its tree.
pub(crate) fn borne_out(receipt: &Receipt, parents: &str, tree: &str) -> bool {
    !receipt.parent.is_empty() && receipt.parent == parents && receipt.tree == tree
}

/// Why `receipt` is not counted for the commit it names on `line`.
pub(crate) fn unborne(receipt: &Receipt, line: &str) -> String {
    format!(
        "the receipt of {} names {} on {line}, whose parent or tree is not the one it recorded \
         (parent {}, tree {}): nothing is counted from a receipt git does not bear out",
        receipt.slug,
        short(&receipt.squash),
        short(&receipt.parent),
        short(&receipt.tree)
    )
}

/// The receipts kept in a repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Receipts {
    all: Vec<Receipt>,
}

impl Receipts {
    /// The receipts of the repository at `root`: each file of the receipts' folder that reads as
    /// the receipt of the task its name is of. A file that does not is no receipt.
    pub(crate) fn read(root: &Path) -> Receipts {
        let Some(lands) = lands(root) else {
            return Receipts::default();
        };
        let Ok(entries) = fs::read_dir(lands.join(RECEIPTS)) else {
            return Receipts::default();
        };
        let mut all: Vec<Receipt> = (entries.filter_map(Result::ok))
            .filter_map(|entry| {
                let path = entry.path();
                let receipt: Receipt =
                    serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
                (Receipt::path(&lands, &receipt.slug) == path).then_some(receipt)
            })
            .collect();
        all.sort_by(|a, b| a.slug.cmp(&b.slug));
        Receipts { all }
    }

    /// These receipts, for a caller that read them elsewhere (a test's).
    #[cfg(test)]
    pub(crate) fn of(all: Vec<Receipt>) -> Receipts {
        Receipts { all }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// The receipt whose squash is `commit`.
    pub(crate) fn squashed(&self, commit: &str) -> Option<&Receipt> {
        self.all.iter().find(|receipt| receipt.squash == commit)
    }

    /// Whether `receipt`'s squash is, in `root`, the commit it recorded ([`borne_out`]).
    pub(crate) fn proved(root: &Path, receipt: &Receipt) -> bool {
        git(root, &["log", "-1", "--format=%P%x09%T", &receipt.squash])
            .ok()
            .and_then(|said| {
                let (parents, tree) = said.split_once('\t')?;
                Some(borne_out(receipt, parents, tree))
            })
            .unwrap_or(false)
    }

    /// Whether `tip` holds `commit`, in `root`: `commit` is `tip` or an ancestor of it, or what
    /// it holds was delivered there by a squash, being a head a receipt kept under [`RETAINED`]
    /// or an ancestor of one, whose squash, borne out, is `tip` or an ancestor of it. The second
    /// is how a release candidate advanced to a delivered head, and the release tagged on it, are
    /// on a line no history of which holds them.
    ///
    /// # Errors
    ///
    /// Git could not say, as for a commit it does not know.
    pub(crate) fn holds(&self, root: &Path, commit: &str, tip: &str) -> Result<bool, String> {
        if is_ancestor(root, commit, tip)? {
            return Ok(true);
        }
        if self.is_empty() {
            return Ok(false);
        }
        let listed = git(
            root,
            &[
                "for-each-ref",
                &format!("--contains={commit}"),
                "--format=%(refname) %(objectname)",
                RETAINED.trim_end_matches('/'),
            ],
        )?;
        for line in listed.lines() {
            let Some((name, head)) = line.split_once(' ') else {
                continue;
            };
            let slug = name.strip_prefix(RETAINED).unwrap_or_default();
            let kept =
                (self.all.iter()).find(|receipt| receipt.slug == slug && receipt.head == head);
            if let Some(receipt) = kept
                && Receipts::proved(root, receipt)
                && is_ancestor(root, &receipt.squash, tip)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_tree::Tree;

    /// `git ARGS` in `root` as a fixture commits, which must succeed: what it printed.
    pub(crate) fn run(root: &Path, args: &[&str]) -> String {
        let named = ["-c", "user.name=t", "-c", "user.email=t@example.com"];
        git(root, &[&named[..], args].concat()).unwrap()
    }

    /// A repository on `main` with one commit.
    pub(crate) fn repository(tree: &Tree) -> PathBuf {
        let root = tree.path("repo");
        fs::create_dir_all(&root).unwrap();
        run(&root, &["init", "-q", "-b", "main"]);
        run(&root, &["commit", "-q", "--allow-empty", "-m", "first"]);
        root
    }

    /// The branch `slug` made at `from` with one commit writing a file of its own, then squashed
    /// onto `main` as a host does, its head kept under [`RETAINED`] and its receipt kept as the
    /// land keeps it: the receipt.
    pub(crate) fn deliver(root: &Path, slug: &str, from: &str) -> Receipt {
        run(root, &["checkout", "-q", "-b", slug, from]);
        let file = format!("{}.txt", slug.replace('/', "-"));
        fs::write(root.join(&file), slug).unwrap();
        run(root, &["add", &file]);
        run(root, &["commit", "-q", "-m", &format!("work on {slug}")]);
        let head = run(root, &["rev-parse", "HEAD"]);
        run(root, &["checkout", "-q", "main"]);
        let receipt = squash(root, slug, &head);
        keep(root, &receipt);
        receipt
    }

    /// `head` squashed onto `main` in one commit of one parent: the receipt of it, not kept.
    pub(crate) fn squash(root: &Path, slug: &str, head: &str) -> Receipt {
        let parent = run(root, &["rev-parse", "main"]);
        run(root, &["merge", "-q", "--squash", head]);
        run(
            root,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("{slug}: its title"),
            ],
        );
        Receipt {
            slug: slug.into(),
            task: 7,
            operation: "task".into(),
            repository: 11,
            name: "owner/repository".into(),
            pull: 5,
            head: head.into(),
            base_observed: parent.clone(),
            parent,
            squash: run(root, &["rev-parse", "HEAD"]),
            tree: run(root, &["rev-parse", "HEAD^{tree}"]),
            revision: "r1".into(),
            by: "lander".into(),
        }
    }

    /// `receipt` kept in `root` as the land keeps it, its head retained.
    pub(crate) fn keep(root: &Path, receipt: &Receipt) {
        let path = Receipt::path(&lands(root).unwrap(), &receipt.slug);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_string_pretty(receipt).unwrap()).unwrap();
        let retained = format!("{RETAINED}{}", receipt.slug);
        run(root, &["update-ref", &retained, &receipt.head]);
    }

    #[test]
    fn the_receipts_read_are_the_files_that_are_their_own_tasks_and_no_other() {
        let tree = Tree::new();
        let root = repository(&tree);
        assert!(Receipts::read(&root).is_empty(), "no folder, no receipt");
        assert!(Receipts::read(&tree.path("nowhere")).is_empty());
        let one = deliver(&root, "tools/one", "main");
        let two = deliver(&root, "app/two", "main");
        let folder = lands(&root).unwrap().join(RECEIPTS);
        // A file that does not read, and a receipt under another task's name, are none.
        fs::write(folder.join("torn.json"), "{\"slug\": ").unwrap();
        fs::write(
            folder.join("tools%2Fother.json"),
            serde_json::to_string(&one).unwrap(),
        )
        .unwrap();
        let read = Receipts::read(&root);
        assert_eq!(read, Receipts::of(vec![two.clone(), one.clone()]));
        assert_eq!(read.squashed(&one.squash), Some(&one));
        assert_eq!(
            read.squashed(&one.head),
            None,
            "by its squash, never its head"
        );
    }

    #[test]
    fn a_receipt_is_borne_out_by_the_one_parent_and_the_tree_it_recorded() {
        let tree = Tree::new();
        let root = repository(&tree);
        let receipt = deliver(&root, "tools/one", "main");
        assert!(borne_out(&receipt, &receipt.parent, &receipt.tree));
        assert!(!borne_out(&receipt, &receipt.parent, &receipt.parent));
        assert!(!borne_out(&receipt, "", &receipt.tree), "a root commit");
        let two = format!("{} {}", receipt.parent, receipt.head);
        assert!(!borne_out(&receipt, &two, &receipt.tree), "a merge commit");
        let blank = Receipt {
            parent: String::new(),
            ..receipt.clone()
        };
        assert!(!borne_out(&blank, "", &receipt.tree));
        assert!(Receipts::proved(&root, &receipt));
        let wrong = Receipt {
            tree: receipt.parent.clone(),
            ..receipt.clone()
        };
        assert!(!Receipts::proved(&root, &wrong));
        let gone = Receipt {
            squash: "f".repeat(40),
            ..receipt.clone()
        };
        assert!(!Receipts::proved(&root, &gone));
        assert_eq!(
            unborne(&receipt, "origin/main"),
            format!(
                "the receipt of tools/one names {} on origin/main, whose parent or tree is not \
                 the one it recorded (parent {}, tree {}): nothing is counted from a receipt git \
                 does not bear out",
                short(&receipt.squash),
                short(&receipt.parent),
                short(&receipt.tree)
            )
        );
    }

    #[test]
    fn a_line_holds_its_ancestors_and_what_a_borne_out_squash_delivered_there() {
        let tree = Tree::new();
        let root = repository(&tree);
        let first = run(&root, &["rev-parse", "main"]);
        let one = deliver(&root, "tools/one", "main");
        let after_one = run(&root, &["rev-parse", "main"]);
        // A second head built on the first one's, as a release candidate's second fix is.
        let two = deliver(&root, "tools/two", &one.head);
        let tip = run(&root, &["rev-parse", "main"]);
        let receipts = Receipts::read(&root);
        assert_eq!(receipts.holds(&root, &first, &tip), Ok(true), "an ancestor");
        assert_eq!(receipts.holds(&root, &tip, &tip), Ok(true), "itself");
        assert_eq!(receipts.holds(&root, &one.head, &tip), Ok(true));
        assert_eq!(receipts.holds(&root, &two.head, &tip), Ok(true));
        // Held from its own squash on, never before it.
        assert_eq!(receipts.holds(&root, &two.head, &after_one), Ok(false));
        assert_eq!(receipts.holds(&root, &one.head, &after_one), Ok(true));
        assert_eq!(receipts.holds(&root, &one.head, &first), Ok(false));
        // With no receipt, a head no history holds is not held; nor is one whose receipt git
        // does not bear out, or whose retained reference names another commit.
        assert_eq!(Receipts::default().holds(&root, &two.head, &tip), Ok(false));
        let wrong = Receipt {
            tree: two.parent.clone(),
            ..two.clone()
        };
        let unborne = Receipts::of(vec![wrong]);
        assert_eq!(unborne.holds(&root, &two.head, &tip), Ok(false));
        // A branch that was never delivered is held by nothing.
        run(&root, &["checkout", "-q", "-b", "stray", &first]);
        run(&root, &["commit", "-q", "--allow-empty", "-m", "stray"]);
        let stray = run(&root, &["rev-parse", "HEAD"]);
        run(&root, &["checkout", "-q", "main"]);
        assert_eq!(receipts.holds(&root, &stray, &tip), Ok(false));
        let moved = format!("{RETAINED}tools/two");
        run(&root, &["update-ref", &moved, &stray]);
        assert_eq!(receipts.holds(&root, &two.head, &tip), Ok(false));
        assert_eq!(receipts.holds(&root, &stray, &tip), Ok(false));
        assert!(receipts.holds(&root, "nothing-here", &tip).is_err());
    }
}
