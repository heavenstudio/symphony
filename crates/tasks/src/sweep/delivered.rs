//! A head a pull request squashed into main, as the sweep proves it: the second proof of a landing,
//! beside ancestry. The host's squash leaves the task's head in no history of main
//! ([`crate::land::delivery`]), so the receipt the land kept once it proved the host's commit is
//! what says the head landed, and only while git and the kept files bear all of it out now:
//!
//! * a [`Receipt`] kept under its own task's name whose `head` is exactly the commit asked about: a
//!   branch that moved on, or an ancestor of a delivered head, is of another head;
//! * that head still kept under [`RETAINED`]`SLUG`, so a branch deleted loses no commit;
//! * its `squash` the commit it recorded: of that id, of the one parent and of the tree it wrote,
//!   that tree being what git makes of merging the head into that parent, as the land proved it;
//! * that squash on the remote default branch as last fetched (`refs/remotes/`[`REMOTE`]`/main`)
//!   and in main here: a delivery the main checkout has not followed yet is not landed here.
//!
//! Anything else is not delivered, and the worktree and branch stay as any unmerged one does: no
//! receipt, one that does not read or is of another task, another head, a retained head gone or
//! moved, a commit that is not the receipt's, a remote that never held it. An import's commit has
//! two parents and its head is in main's history, which ancestry proves. This module reads: it
//! writes no receipt, moves no reference, fetches nothing and asks no host anything. Tests:
//! `crates/tasks/tests/sweep/delivered.rs`.

use super::{git, trigger};
use crate::land::delivery::{Journal, RECEIPTS, RETAINED, Receipt};
use crate::land::{FOLDER, REMOTE};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The squash that delivered a head, as its receipt names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    pub slug: String,
    pub squash: String,
    pub pull: i64,
}

impl Proof {
    /// How a line says it: `squashed into main as S by pull request #N`.
    pub fn said(&self) -> String {
        let short = self.squash.get(..12).unwrap_or(&self.squash);
        format!(
            "squashed into main as {short} by pull request #{}",
            self.pull
        )
    }
}

/// The deliveries of one repository, read once a sweep and only when a head is not in main.
pub struct Delivered {
    root: PathBuf,
    /// The lands' folder; none where `root` is in no repository.
    lands: Option<PathBuf>,
    receipts: Option<Vec<Receipt>>,
    proved: BTreeMap<String, Option<Proof>>,
}

impl Delivered {
    pub fn of(root: &Path) -> Delivered {
        let lands = crate::git::common_dir(root)
            .ok()
            .map(|common| trigger::folder(&common).join(FOLDER));
        Delivered {
            root: root.to_path_buf(),
            lands,
            receipts: None,
            proved: BTreeMap::new(),
        }
    }

    /// The proof that `head` was delivered into `main`, a commit of the default branch here; once
    /// per commit a sweep.
    pub fn proof(&mut self, head: &str, main: &str) -> Option<Proof> {
        if head.is_empty() {
            return None;
        }
        if let Some(known) = self.proved.get(head) {
            return known.clone();
        }
        let root = self.root.clone();
        let proof = (self.receipts().iter())
            .filter(|receipt| receipt.head == head)
            .find(|receipt| borne_out(&root, receipt, main))
            .map(|receipt| Proof {
                slug: receipt.slug.clone(),
                squash: receipt.squash.clone(),
                pull: receipt.pull,
            });
        self.proved.insert(head.to_string(), proof.clone());
        proof
    }

    /// Why the branch `name` stays while a delivery of its task is under way, if one is: the land
    /// run again reads the branch's tip against the pull request it is bound to.
    pub fn under_way(&self, name: &str) -> Option<String> {
        let journal = Journal::read(self.lands.as_deref()?, name)?;
        Some(format!(
            "a delivery of it by pull request is under way, and `just land {}` run again reads \
             its tip",
            journal.slug
        ))
    }

    /// Each file of the receipts' folder that reads as the receipt of the task its name is of.
    fn receipts(&mut self) -> &[Receipt] {
        let lands = self.lands.clone();
        self.receipts.get_or_insert_with(|| {
            let Some(lands) = lands else {
                return Vec::new();
            };
            let Ok(entries) = std::fs::read_dir(lands.join(RECEIPTS)) else {
                return Vec::new();
            };
            let mut read: Vec<Receipt> = (entries.filter_map(Result::ok))
                .filter_map(|entry| {
                    let path = entry.path();
                    let text = std::fs::read_to_string(&path).ok()?;
                    let receipt: Receipt = serde_json::from_str(&text).ok()?;
                    (Receipt::path(&lands, &receipt.slug) == path).then_some(receipt)
                })
                .collect();
            read.sort_by(|a, b| a.slug.cmp(&b.slug));
            read
        })
    }
}

/// Whether git, in `root`, bears out all `receipt` says of its head and `main` holds its squash.
fn borne_out(root: &Path, receipt: &Receipt, main: &str) -> bool {
    let (head, squash, parent) = (&receipt.head, &receipt.squash, &receipt.parent);
    let retained = format!("{RETAINED}{}", receipt.slug);
    if git::rev(root, &retained).as_deref() != Some(head.as_str()) {
        return false;
    }
    // The commit by its whole id, so a receipt naming a branch or a short id proves nothing.
    let recorded = format!("{squash}\t{parent}\t{}", receipt.tree);
    let commit = git::stdout(root, &["log", "-1", "--format=%H%x09%P%x09%T", squash]);
    if parent.is_empty() || commit.as_deref().map(str::trim) != Some(recorded.as_str()) {
        return false;
    }
    let merge = git::stdout(root, &["merge-tree", "--write-tree", parent, head]);
    if merge.as_deref().and_then(|out| out.lines().next()) != Some(receipt.tree.as_str()) {
        return false;
    }
    let tracking = format!("refs/remotes/{REMOTE}/main");
    git::rev(root, &tracking).is_some_and(|remote| git::ancestor(root, squash, &remote))
        && git::ancestor(root, squash, main)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_head_and_a_folder_in_no_repository_prove_nothing() {
        let nowhere =
            std::env::temp_dir().join(format!("symphony-delivered-{}", std::process::id()));
        std::fs::create_dir_all(&nowhere).unwrap();
        let mut delivered = Delivered::of(&nowhere);
        assert_eq!(delivered.proof("", "main"), None);
        assert_eq!(delivered.proof(&"a".repeat(40), "main"), None);
        // Asked again, it is answered from what was found.
        assert_eq!(delivered.proof(&"a".repeat(40), "main"), None);
        assert_eq!(delivered.under_way("tools/any"), None);
        std::fs::remove_dir(&nowhere).unwrap();
    }

    #[test]
    fn a_proof_says_its_squash_short_and_its_pull_request() {
        let proof = |squash: &str| Proof {
            slug: "tools/any".into(),
            squash: squash.into(),
            pull: 9,
        };
        assert_eq!(
            proof(&"b".repeat(40)).said(),
            "squashed into main as bbbbbbbbbbbb by pull request #9"
        );
        assert_eq!(
            proof("abc").said(),
            "squashed into main as abc by pull request #9"
        );
    }
}
