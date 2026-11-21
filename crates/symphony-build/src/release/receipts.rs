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
//!
//! A reader of every receipt at once (a release's notes, [`super::consumer`]) asks git once for
//! all of them ([`Receipts::proof`]), in two processes whatever their number, never once for each
//! receipt or each pair.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use symphony_board::land::delivery::{RECEIPTS, RETAINED, Receipt};

use super::git::{git, git_fed, is_ancestor, short};

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

/// Whether `name` can be asked of git among the others' ([`Receipts::proof`]) without costing
/// them their answers; one that cannot is left out of the ask, and names no commit there, so the
/// receipt giving it alone is unproved. A name of more than one line would shift every answer
/// after it. A reflog or upstream form (`@{u}`, `@{push}`, `HEAD@{99}`) ends git where it has no
/// upstream or no such entry, before it answers any name after it. The land writes neither: it
/// records the commits git gave it in full.
fn asked_alone(name: &str) -> bool {
    !name.contains(['\n', '\0']) && !name.contains("@{")
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

    /// What git says of every receipt, asked once in the repository at `root`: in two processes,
    /// whatever their number. One `cat-file --batch-check` names in full the commit that each
    /// squash, head and parent a receipt gives names, if any. One `log --stdin` then walks every
    /// commit those squashes and parents hold, with its parents and its tree. Nothing is known
    /// where git cannot say, and so nothing is borne out, as [`Receipts::proved`] says; a name
    /// git cannot be asked beside the others ([`asked_alone`]) names nothing here, whatever it
    /// would name alone.
    pub(crate) fn proof(&self, root: &Path) -> Proof {
        if self.is_empty() {
            return Proof::default();
        }
        let mut names: Vec<&str> = (self.all.iter())
            .flat_map(|receipt| [&receipt.squash, &receipt.head, &receipt.parent])
            .map(String::as_str)
            .filter(|name| asked_alone(name))
            .collect();
        names.sort_unstable();
        names.dedup();
        let asked: String = (names.iter())
            .map(|name| format!("{name}^{{commit}}\n"))
            .collect();
        let format = "--batch-check=%(objectname) %(objecttype)";
        // One answer a line, in the order asked: `NAME missing` for a name of no commit. Answers
        // that do not pair with the names asked would bear out the wrong receipts: none is read.
        let answers = git_fed(root, &["cat-file", format], &asked).unwrap_or_default();
        if answers.lines().count() != names.len() {
            return Proof::default();
        }
        let named: HashMap<String, String> = (names.iter().zip(answers.lines()))
            .filter_map(|(name, answer)| {
                let commit = answer.strip_suffix(" commit")?;
                Some((name.to_string(), commit.to_string()))
            })
            .collect();
        let mut starts: Vec<&str> = (self.all.iter())
            .flat_map(|receipt| [&receipt.squash, &receipt.parent])
            .filter_map(|name| named.get(name.as_str()))
            .map(String::as_str)
            .collect();
        if starts.is_empty() {
            return Proof {
                named,
                ..Proof::default()
            };
        }
        starts.sort_unstable();
        starts.dedup();
        // Each line a commit in full that git just named, never an option or a name of none, and
        // at least one, so the walk never falls back to `HEAD`.
        let walk: String = starts.iter().map(|commit| format!("{commit}\n")).collect();
        let Ok(walked) = git_fed(root, &["log", "--stdin", "--format=%H%x09%P%x09%T"], &walk)
        else {
            return Proof::default();
        };
        let rows: Vec<(&str, &str, &str)> = (walked.lines())
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                Some((fields.next()?, fields.next()?, fields.next()?))
            })
            .collect();
        let nodes: HashMap<String, usize> = (rows.iter().enumerate())
            .map(|(node, (commit, _, _))| ((*commit).to_string(), node))
            .collect();
        let commits = (rows.iter())
            .map(|(_, parents, tree)| Walked {
                parents: (*parents).to_string(),
                tree: (*tree).to_string(),
                up: (parents.split(' '))
                    .filter_map(|parent| nodes.get(parent).copied())
                    .collect(),
            })
            .collect();
        Proof {
            named,
            nodes,
            commits,
        }
    }

    /// The head of each other receipt git bears out whose squash the base `receipt`'s was made on
    /// holds, where that head is still in the repository: what landed on the line before
    /// `receipt` did, by `proof`, these receipts' own ([`Receipts::proof`]). A head started at
    /// one of them (a release candidate's second fix, started where the first left the
    /// candidate) holds that one's commits, which no history of the line does, so what is read
    /// from `receipt`'s head as its own leaves these out.
    pub(crate) fn landed_before(&self, proof: &Proof, receipt: &Receipt) -> Vec<&str> {
        let held = proof.held(&receipt.parent);
        (self.all.iter())
            .filter(|other| {
                other.squash != receipt.squash
                    && proof.proved(other)
                    && proof.node(&other.squash).is_some_and(|node| held[node])
                    && proof.named.contains_key(&other.head)
            })
            .map(|other| other.head.as_str())
            .collect()
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

/// What git said of a set of receipts, asked once ([`Receipts::proof`]).
#[derive(Debug, Default)]
pub(crate) struct Proof {
    /// Each name a receipt gives that names a commit, with that commit in full.
    named: HashMap<String, String>,
    /// Each commit the walk read, in full, with its place in `commits`.
    nodes: HashMap<String, usize>,
    commits: Vec<Walked>,
}

/// A commit the walk read.
#[derive(Debug)]
struct Walked {
    /// Its parents as git says them (`%P`), and its tree.
    parents: String,
    tree: String,
    /// Its parents' places among the commits the walk read.
    up: Vec<usize>,
}

impl Proof {
    /// The place of the commit `name` names among those the walk read.
    fn node(&self, name: &str) -> Option<usize> {
        self.nodes.get(self.named.get(name)?).copied()
    }

    /// Whether `receipt`'s squash is the commit it recorded ([`borne_out`]): what
    /// [`Receipts::proved`] says, without asking git again.
    pub(crate) fn proved(&self, receipt: &Receipt) -> bool {
        self.node(&receipt.squash).is_some_and(|node| {
            let squash = &self.commits[node];
            borne_out(receipt, &squash.parents, &squash.tree)
        })
    }

    /// For each commit the walk read, whether the commit `name` names holds it: is it or has it
    /// among its ancestors. None does where `name` names no commit.
    fn held(&self, name: &str) -> Vec<bool> {
        let mut held = vec![false; self.commits.len()];
        let mut next: Vec<usize> = self.node(name).into_iter().collect();
        while let Some(node) = next.pop() {
            if !std::mem::replace(&mut held[node], true) {
                next.extend(&self.commits[node].up);
            }
        }
        held
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
            import: None,
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

    #[test]
    fn what_landed_before_a_receipt_is_each_borne_out_squash_its_base_holds_whose_head_is_there() {
        let tree = Tree::new();
        let root = repository(&tree);
        let one = deliver(&root, "tools/one", "main");
        let two = deliver(&root, "tools/two", &one.head);
        let three = deliver(&root, "tools/three", "main");
        let receipts = Receipts::read(&root);
        let proof = receipts.proof(&root);
        assert!(receipts.landed_before(&proof, &one).is_empty(), "the first");
        assert_eq!(receipts.landed_before(&proof, &two), [one.head.as_str()]);
        // Oldest slug first, as they are read; never itself, nor what landed after it.
        assert_eq!(
            receipts.landed_before(&proof, &three),
            [one.head.as_str(), two.head.as_str()]
        );
        // One git does not bear out, and one whose head is gone, are left out.
        let wrong = Receipt {
            tree: one.parent.clone(),
            ..one.clone()
        };
        let gone = Receipt {
            head: "f".repeat(40),
            ..two.clone()
        };
        let partly = Receipts::of(vec![wrong, gone, three.clone()]);
        assert!(
            partly
                .landed_before(&partly.proof(&root), &three)
                .is_empty()
        );
        // One that names its squash and head otherwise than in full is read as git reads it.
        let short = Receipt {
            squash: one.squash[..12].to_string(),
            head: "tools/one^0".into(),
            ..one.clone()
        };
        let named = Receipts::of(vec![short, three.clone()]);
        assert_eq!(
            named.landed_before(&named.proof(&root), &three),
            ["tools/one^0"]
        );
        // A base git does not know holds nothing.
        let nowhere = Receipt {
            parent: "f".repeat(40),
            ..three.clone()
        };
        assert!(receipts.landed_before(&proof, &nowhere).is_empty());
    }

    /// The proof says of each receipt what [`Receipts::proved`] says of it alone, whatever its
    /// squash names: a commit in full or abbreviated, a tag of it, a branch of another commit, a
    /// tree, nothing, a name of two lines. It takes two git processes for all of them, none for
    /// no receipt, and one where no name names a commit.
    #[test]
    fn the_proof_bears_out_what_proved_does_for_every_receipt_at_once() {
        let tree = Tree::new();
        let root = repository(&tree);
        let one = deliver(&root, "tools/one", "main");
        let two = deliver(&root, "tools/two", &one.head);
        run(
            &root,
            &["tag", "-a", "squashed", "-m", "squashed", &two.squash],
        );
        let with = |squash: &str| Receipt {
            squash: squash.into(),
            ..two.clone()
        };
        let cases = [
            one.clone(),
            two.clone(),
            with(&two.squash[..10]),
            with("squashed"),
            with("tools/two"),
            with(&two.tree),
            with(""),
            with(&format!("{}\n{}", two.squash, two.squash)),
            with(&"f".repeat(40)),
            Receipt {
                tree: one.parent.clone(),
                ..one.clone()
            },
            Receipt {
                parent: String::new(),
                ..one.clone()
            },
        ];
        let receipts = Receipts::of(cases.to_vec());
        let from = crate::release::git::ran();
        let proof = receipts.proof(&root);
        assert_eq!(
            crate::release::git::ran() - from,
            2,
            "two processes for all"
        );
        let alone: Vec<bool> = (cases.iter())
            .map(|receipt| Receipts::proved(&root, receipt))
            .collect();
        let at_once: Vec<bool> = cases.iter().map(|receipt| proof.proved(receipt)).collect();
        assert_eq!(at_once, alone);
        assert_eq!(
            at_once,
            [
                true, true, true, true, false, false, false, false, false, false, false
            ]
        );

        let from = crate::release::git::ran();
        let none = Receipts::default().proof(&root);
        assert_eq!(crate::release::git::ran() - from, 0, "no receipt, no git");
        assert!(!none.proved(&one));
        // Where git cannot say, nothing is borne out.
        assert!(!receipts.proof(&tree.path("nowhere")).proved(&one));
        // Where no name names a commit, nothing is walked.
        let unknown = Receipts::of(vec![Receipt {
            squash: "f".repeat(40),
            head: "f".repeat(40),
            parent: "e".repeat(40),
            ..two.clone()
        }]);
        let from = crate::release::git::ran();
        assert!(!unknown.proof(&root).proved(&two));
        assert_eq!(crate::release::git::ran() - from, 1);
    }

    /// A receipt naming a reflog or upstream form, which ends git's one answering of every name
    /// where it has no upstream or no such entry, is unproved alone: every other receipt is
    /// proved as before, in the same two processes.
    #[test]
    fn a_receipt_naming_a_reflog_or_upstream_form_is_unproved_and_the_others_stand() {
        let tree = Tree::new();
        let root = repository(&tree);
        let one = deliver(&root, "tools/one", "main");
        let two = deliver(&root, "tools/two", &one.head);
        let three = deliver(&root, "tools/three", "main");
        let squashed = |slug: &str, squash: &str| Receipt {
            slug: slug.into(),
            squash: squash.into(),
            ..three.clone()
        };
        let upstream = squashed("odd/upstream", "@{u}");
        let reflog = squashed("odd/reflog", "HEAD@{99}");
        // One whose head or base is such a name keeps what its other names say.
        let headed = Receipt {
            head: "@{push}".into(),
            ..two.clone()
        };
        let based = Receipt {
            parent: "main@{upstream}".into(),
            ..three.clone()
        };
        let cases = [
            one.clone(),
            upstream.clone(),
            headed.clone(),
            reflog.clone(),
            three.clone(),
            based.clone(),
        ];
        let receipts = Receipts::of(cases.to_vec());
        let from = crate::release::git::ran();
        let proof = receipts.proof(&root);
        assert_eq!(crate::release::git::ran() - from, 2, "one ask, one walk");
        let at_once: Vec<bool> = cases.iter().map(|receipt| proof.proved(receipt)).collect();
        assert_eq!(at_once, [true, false, true, false, true, false]);
        // Each is what git says of it alone, where git ends on these names too.
        let alone: Vec<bool> = (cases.iter())
            .map(|receipt| Receipts::proved(&root, receipt))
            .collect();
        assert_eq!(at_once, alone);
        // What landed before is still read: a head so named is in no repository, and is left out.
        assert_eq!(receipts.landed_before(&proof, &three), [one.head.as_str()]);
        assert!(receipts.landed_before(&proof, &upstream).len() == 1);
        // A reflog name git would answer is left out all the same, and so unproved here.
        let answered = squashed("odd/answered", "HEAD@{0}");
        assert!(Receipts::proved(&root, &answered));
        let with = Receipts::of(vec![one.clone(), answered.clone()]);
        let proof = with.proof(&root);
        assert!(proof.proved(&one) && !proof.proved(&answered));
        for name in ["@{u}", "HEAD@{99}", "main@{push}", "a\nb", "a\0b"] {
            assert!(!asked_alone(name), "{name:?}");
        }
        for name in [
            one.squash.as_str(),
            "tools/one^0",
            "squashed",
            "",
            "@",
            "a{b}",
        ] {
            assert!(asked_alone(name), "{name:?}");
        }
    }
}
