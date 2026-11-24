//! Native saves and their background work, on private repositories and a private board per case.
include!("common/commit_fixture.rs");
use symphony_board::{rpc, sys};

#[path = "after_commit/delivery.rs"]
mod delivery;
#[path = "after_commit/publication_reports.rs"]
mod publication_reports;
#[path = "after_commit/settings.rs"]
mod settings;
#[path = "after_commit/uploads.rs"]
mod uploads;

#[test]
fn an_owner_save_commits_the_uploads_its_new_line_names() {
    isolated(
        "an_owner_save_commits_the_uploads_its_new_line_names",
        uploads::an_owner_save_commits_the_uploads_its_new_line_names,
    );
}

#[test]
fn under_another_artifacts_folder_the_upload_is_committed_where_its_link_points() {
    isolated(
        "under_another_artifacts_folder_the_upload_is_committed_where_its_link_points",
        uploads::under_another_artifacts_folder_the_upload_is_committed_where_its_link_points,
    );
}

#[test]
fn a_store_file_that_is_not_its_upload_is_left_out_and_the_verdict_saved() {
    isolated(
        "a_store_file_that_is_not_its_upload_is_left_out_and_the_verdict_saved",
        uploads::a_store_file_that_is_not_its_upload_is_left_out_and_the_verdict_saved,
    );
}

#[test]
fn a_generated_verdict_branch_carries_the_uploads_its_line_names() {
    isolated(
        "a_generated_verdict_branch_carries_the_uploads_its_line_names",
        uploads::a_generated_verdict_branch_carries_the_uploads_its_line_names,
    );
}

#[test]
fn scratch_cli_build_keeps_the_main_heartbeat_link() {
    let began = Instant::now();
    stage("scratch CLI fixture setup started");
    let tree = common::Tree::new();
    let board = tree.path("stand-in-board");
    let main = tree.path("main-tasks");
    let branch = tree.path("branch-tasks");
    let heartbeat = tree.path(".target/bin/tasks");
    std::fs::create_dir_all(heartbeat.parent().unwrap()).unwrap();
    prepared_script(&main, &format!("printf main > {}", quote(&board)));
    prepared_script(
        &branch,
        &format!(
            "[ \"$1\" = --database ] && [ \"$2\" = scratch ]\nprintf branch > {}",
            quote(&board)
        ),
    );
    std::os::unix::fs::symlink(&main, &heartbeat).unwrap();
    prepared_script(
        &tree.path("tools/rs-bin.sh"),
        &format!(
            "printf '%s\\n' \"$*\" >> {}\ncase \"$1\" in\n--link) ln -sfn {} {};;\n--build|--where) printf '%s\\n' {};;\n*) exit 91;;\nesac",
            quote(&tree.path("launcher-calls")),
            quote(&branch),
            quote(&heartbeat),
            quote(&branch)
        ),
    );
    // These three commands share assessed executable bytes but retain distinct
    // bodies and argv[0]; the heartbeat below also exercises an alias to one.
    use std::os::unix::fs::MetadataExt;
    let inode = std::fs::metadata(&main).unwrap().ino();
    assert_eq!(std::fs::metadata(&branch).unwrap().ino(), inode);
    assert_eq!(
        std::fs::metadata(tree.path("tools/rs-bin.sh"))
            .unwrap()
            .ino(),
        inode
    );

    stage("scratch CLI launcher started");
    let binary = common::build_tasks(&tree.root);
    stage("scratch CLI launcher completed; running branch CLI");
    assert_eq!(Path::new(&binary), branch);
    assert_eq!(tree.read("launcher-calls"), "--build tasks\n");
    assert_eq!(std::fs::read_link(&heartbeat).unwrap(), main);
    let done = sys::output(sys::Command::new(binary).args(["--database", "scratch"])).unwrap();
    assert!(done.status.success(), "{done:?}");
    assert_eq!(tree.read("stand-in-board"), "branch");
    stage("branch CLI completed; running heartbeat CLI");
    assert!(Command::new(heartbeat).status_locked().unwrap().success());
    assert_eq!(tree.read("stand-in-board"), "main");
    stage(format_args!(
        "scratch CLI case completed, wall={:.3}s",
        began.elapsed().as_secs_f64()
    ));
}

/// The scratch CLI's first run, which macOS assesses, happens in setup: once, as `--help`, with
/// a bare environment that names no board; a CLI that cannot answer stops the setup there.
#[test]
fn the_scratch_cli_is_run_once_bare_before_any_case_runs_it() {
    let tree = common::Tree::new();
    let cli = tree.path("cli");
    prepared_script(
        &cli,
        &format!(
            "printf '%s\\n' \"$*\" >> {said}\nenv >> {said}",
            said = quote(&tree.path("said"))
        ),
    );
    common::run_once(&cli);
    let said = tree.read("said");
    let mut lines = said.lines();
    assert_eq!(lines.next(), Some("--help"), "{said}");
    let env: Vec<_> = lines.collect();
    assert!(env.contains(&"PATH=/usr/bin:/bin"), "{said}");
    assert!(
        !env.iter()
            .any(|line| ["HOME=", "TASKS_", "SYMPHONY_", "GIT_"]
                .iter()
                .any(|prefix| line.starts_with(prefix))),
        "{said}"
    );

    let broken = tree.path("broken-cli");
    prepared_script(&broken, "echo 'no such board' >&2\nexit 3");
    let refused = std::panic::catch_unwind(|| common::run_once(&broken)).unwrap_err();
    let message = refused.downcast_ref::<String>().unwrap();
    assert!(
        message.contains(&format!("{} --help", broken.display()))
            && message.contains("no such board"),
        "{message}"
    );
}

#[test]
fn child_stages_are_forwarded_before_the_child_closes_its_output() {
    struct InterruptedOnce<R> {
        pending: bool,
        reader: R,
    }
    impl<R: Read> Read for InterruptedOnce<R> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if std::mem::take(&mut self.pending) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            self.reader.read(bytes)
        }
    }
    struct Chunks(std::sync::mpsc::Sender<Vec<u8>>);
    impl Write for Chunks {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.send(bytes.to_vec()).unwrap();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    // A pipe no git another test spawns meanwhile can keep: the relay's end comes with `child`'s.
    let (reader, mut child) = sys::pipe().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let forwarding = std::thread::spawn(move || {
        let mut saved = Vec::new();
        relay_output(
            InterruptedOnce {
                pending: true,
                reader,
            },
            &mut saved,
            Chunks(send),
        )
        .unwrap();
        saved
    });
    let mut observed = Vec::new();
    for chunk in [b"awaiting gate\n".as_slice(), b"filing\xffstill live\n"] {
        child.write_all(chunk).unwrap();
        let end = observed.len() + chunk.len();
        while observed.len() < end {
            observed.extend(receive.recv().unwrap());
        }
        assert_eq!(&observed[end - chunk.len()..], chunk);
    }
    drop(child);
    assert_eq!(forwarding.join().unwrap(), observed);
}

#[test]
fn commits_only_immutable_edits_and_preserves_the_real_index() {
    isolated(
        "commits_only_immutable_edits_and_preserves_the_real_index",
        |f| {
            let root = f.repo();
            let verdict = DOC.replace("[ ]", "[x]");
            let noted = verdict.replace("last paragraph", "last paragraph, noted");
            let first = f.edit("docs/a.md", DOC, &verdict);
            let second = f.edit("docs/a.md", &verdict, &noted);
            let local = noted.replace("first paragraph", "first paragraph, still local");
            write(&root.join("docs/a.md"), &local);
            write(&root.join("other"), "someone else's staged work\n");
            git(&root, &["add", "other"]);
            let before_index = git(&root, &["ls-files", "-s", "other"]);
            let outcome = commit_verdicts(&root, &[first, second], "docs: owner verdict");
            assert!(
                outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                "{} {:?}",
                outcome.error,
                outcome.notes
            );
            assert_eq!(f.main("docs/a.md"), noted);
            assert_eq!(f.main("other"), "original\n");
            assert_eq!(
                git(
                    &root,
                    &["diff-tree", "--no-commit-id", "--name-only", "-r", "main"]
                ),
                "docs/a.md"
            );
            assert_eq!(git(&root, &["ls-files", "-s", "other"]), before_index);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                local
            );
            let tip = git(&root, &["rev-parse", "main"]);
            let noop = commit_verdicts(
                &root,
                &[Edit {
                    rel: "docs/a.md".into(),
                    base: noted.clone(),
                    new: noted,
                }],
                "no change",
            );
            assert!(!noop.committed && noop.error.is_empty());
            assert_eq!(git(&root, &["rev-parse", "main"]), tip);
            f.clean_workers();
        },
    );
}

#[test]
fn a_save_under_delivery_is_kept_pending_on_a_branch_and_filed_as_the_owners_task() {
    isolated(
        "a_save_under_delivery_is_kept_pending_on_a_branch_and_filed_as_the_owners_task",
        delivery::kept_pending_on_a_branch_and_filed_as_the_owners_task,
    );
}

#[test]
fn a_persons_save_under_delivery_is_filed_and_sent_to_review_as_theirs() {
    isolated(
        "a_persons_save_under_delivery_is_filed_and_sent_to_review_as_theirs",
        delivery::a_persons_save_is_filed_and_sent_to_review_as_theirs,
    );
}

#[test]
fn pending_saves_a_then_b_go_through_delivery_cancellation_cover_and_restart() {
    isolated(
        "pending_saves_a_then_b_go_through_delivery_cancellation_cover_and_restart",
        delivery::a_then_b_through_delivery_cancellation_and_restart,
    );
}

#[test]
fn a_save_journaled_before_delivery_was_set_and_one_that_no_longer_applies_are_kept() {
    isolated(
        "a_save_journaled_before_delivery_was_set_and_one_that_no_longer_applies_are_kept",
        delivery::journaled_before_the_key_and_no_longer_applying,
    );
}

#[test]
fn a_pending_save_settled_on_its_branch_is_proved_by_its_receipt_alone() {
    isolated(
        "a_pending_save_settled_on_its_branch_is_proved_by_its_receipt_alone",
        delivery::a_settled_conflict_is_proved_by_its_receipt_alone,
    );
}

#[test]
fn an_agents_reply_kept_from_main_is_delivered_behind_the_owners_save_it_answers() {
    isolated(
        "an_agents_reply_kept_from_main_is_delivered_behind_the_owners_save_it_answers",
        delivery::an_agents_reply_kept_from_main_is_delivered_behind_the_owners_save,
    );
}

#[test]
fn an_agents_saves_whose_turn_has_come_go_as_one_task() {
    isolated(
        "an_agents_saves_whose_turn_has_come_go_as_one_task",
        delivery::an_agents_saves_whose_turn_has_come_go_as_one_task,
    );
}

#[test]
fn a_batch_made_and_never_journaled_is_taken_up_as_it_was() {
    isolated(
        "a_batch_made_and_never_journaled_is_taken_up_as_it_was",
        delivery::a_batch_made_and_never_journaled_is_taken_up_as_it_was,
    );
}

#[test]
fn a_tidys_saves_reach_main_in_one_commit_or_not_at_all() {
    isolated(
        "a_tidys_saves_reach_main_in_one_commit_or_not_at_all",
        delivery::a_tidys_saves_reach_main_in_one_commit_or_not_at_all,
    );
}

#[test]
fn a_tidy_that_cannot_be_committed_holds_back_no_later_save() {
    isolated(
        "a_tidy_that_cannot_be_committed_holds_back_no_later_save",
        delivery::a_tidy_that_cannot_be_committed_holds_back_no_later_save,
    );
}

#[test]
fn the_land_of_a_pending_saves_task_is_said_from_its_journal() {
    isolated(
        "the_land_of_a_pending_saves_task_is_said_from_its_journal",
        delivery::the_land_of_a_pending_saves_task_is_said_from_its_journal,
    );
}

#[test]
fn a_stored_preference_holds_at_once_with_the_store_as_its_source() {
    isolated(
        "a_stored_preference_holds_at_once_with_the_store_as_its_source",
        settings::each_preference_holds_at_once_with_the_store_as_its_source,
    );
}

#[test]
fn an_agents_switch_is_kept_and_made_the_projects_but_never_the_defaults() {
    isolated(
        "an_agents_switch_is_kept_and_made_the_projects_but_never_the_defaults",
        settings::an_agents_switch_is_kept_and_made_the_projects_but_never_the_defaults,
    );
}

#[test]
fn a_stored_value_made_the_projects_is_one_commit_and_its_entry_goes() {
    isolated(
        "a_stored_value_made_the_projects_is_one_commit_and_its_entry_goes",
        settings::a_stored_value_made_the_projects_is_one_commit_and_its_entry_goes,
    );
}

#[test]
fn a_red_settings_check_commits_nothing_and_keeps_the_entry() {
    isolated(
        "a_red_settings_check_commits_nothing_and_keeps_the_entry",
        settings::a_red_settings_check_commits_nothing_and_keeps_the_entry,
    );
}

#[test]
fn under_delivery_a_settings_commit_waits_until_its_delivery_is_proved() {
    isolated(
        "under_delivery_a_settings_commit_waits_until_its_delivery_is_proved",
        settings::under_delivery_a_commit_waits_on_its_branch_until_its_delivery_is_proved,
    );
}

#[test]
fn under_delivery_settings_made_the_projects_in_turn_each_deliver() {
    isolated(
        "under_delivery_settings_made_the_projects_in_turn_each_deliver",
        settings::under_delivery_settings_made_the_projects_in_turn_each_deliver,
    );
}

#[test]
fn two_verdicts_race_without_lost_files_or_merge_commits() {
    isolated(
        "two_verdicts_race_without_lost_files_or_merge_commits",
        |f| {
            let root = f.repo();
            let base = git(&root, &["rev-parse", "main"]);
            let approved = DOC.replace("[ ]", "[x]");
            let noted = TWO.replace("second proposal.", "second proposal, racing.");
            let edits = [
                f.edit("docs/a.md", DOC, &approved),
                f.edit("docs/b.md", TWO, &noted),
            ];
            write(&root.join("other"), "unrelated staged work\n");
            git(&root, &["add", "other"]);
            let staged = git(&root, &["ls-files", "-s", "other"]);
            let start = Arc::new(Barrier::new(2));
            let workers: Vec<_> = edits
                .into_iter()
                .map(|edit| {
                    let start = start.clone();
                    let root = root.clone();
                    std::thread::spawn(move || {
                        start.wait();
                        commit_verdicts(&root, &[edit], "docs: racing verdict")
                    })
                })
                .collect();
            for worker in workers {
                let outcome = worker.join().unwrap();
                assert!(
                    outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                    "{} {:?}",
                    outcome.error,
                    outcome.notes
                );
            }
            assert_eq!(f.main("docs/a.md"), approved);
            assert_eq!(f.main("docs/b.md"), noted);
            assert_eq!(f.main("other"), "original\n");
            assert_eq!(git(&root, &["ls-files", "-s", "other"]), staged);
            assert_linear_main(&root, &base, 2);
            let mut changed: Vec<_> = git(
                &root,
                &["log", "--format=", "--name-only", &format!("{base}..main")],
            )
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
            changed.sort();
            assert_eq!(changed, ["docs/a.md", "docs/b.md"]);
            f.clean_workers();
        },
    );
}

#[test]
fn stale_branch_edits_merge_into_main_without_branch_prose() {
    isolated(
        "stale_branch_edits_merge_into_main_without_branch_prose",
        |f| {
            let root = f.repo();
            let branch = f.home.join("branch");
            git(
                &root,
                &["worktree", "add", "-qb", "work", branch.to_str().unwrap()],
            );
            let branch_text = DOC.replace("first paragraph", "first paragraph\n\nBRANCH ONLY");
            write(&branch.join("docs/a.md"), &branch_text);
            git(&branch, &["commit", "-qam", "proposal"]);
            let branch_tip = git(&branch, &["rev-parse", "HEAD"]);
            let newer = format!("{DOC}\nnew card on main\n");
            write(&root.join("docs/a.md"), &newer);
            git(&root, &["commit", "-qam", "main gains text"]);
            write(
                &root.join("docs/a.md"),
                &format!("{newer}\nlocal main edit\n"),
            );
            let new = branch_text.replace("[ ]", "[x]");
            write(&branch.join("docs/a.md"), &new);
            let outcome = commit_verdicts(
                &branch,
                &[Edit {
                    rel: "docs/a.md".into(),
                    base: branch_text,
                    new: new.clone(),
                }],
                "docs: owner verdict",
            );
            assert!(
                outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                "{} {:?}",
                outcome.error,
                outcome.notes
            );
            assert_eq!(f.main("docs/a.md"), newer.replace("[ ]", "[x]"));
            assert_eq!(git(&branch, &["rev-parse", "HEAD"]), branch_tip);
            assert_eq!(
                std::fs::read_to_string(branch.join("docs/a.md")).unwrap(),
                new
            );
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                format!("{}\nlocal main edit\n", newer.replace("[ ]", "[x]"))
            );
            assert!(git(&root, &["diff", "--cached", "--name-only"]).is_empty());
            git(
                &root,
                &["worktree", "remove", "--force", branch.to_str().unwrap()],
            );
            f.clean_workers();
        },
    );
}

#[test]
fn syncing_queued_stale_saves_beside_a_new_card_reports_no_failure() {
    isolated(
        "syncing_queued_stale_saves_beside_a_new_card_reports_no_failure",
        |f| {
            use symphony::edit::add_message;
            use symphony::register::Who;
            let root = f.repo();
            let base = "- [ ] **one** — Existing card.\n";
            let fresh = "- [ ] **fresh** — New adjacent card.\n";
            for before in [false, true] {
                write(&root.join("docs/a.md"), base);
                git(&root, &["commit", "-qam", "base card"]);
                let current = add_message(
                    base,
                    "one",
                    "Concurrent reply.",
                    "2026-10-30 07:41",
                    Who::Agent,
                    None,
                )
                .unwrap();
                let current = if before {
                    format!("{fresh}\n{current}")
                } else {
                    format!("{current}\n{fresh}")
                };
                write(&root.join("docs/a.md"), &current);
                git(&root, &["commit", "-qam", "main adds a card and reply"]);
                let saved = add_message(
                    base,
                    "one",
                    "First saved words.",
                    "2026-10-30 07:42",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let queued = add_message(
                    &saved,
                    "one",
                    "Second queued words.",
                    "2026-10-30 07:43",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let edit = f.edit("docs/a.md", base, &saved);
                write(&root.join("docs/a.md"), &queued);
                let committed = add_message(
                    &current,
                    "one",
                    "First saved words.",
                    "2026-10-30 07:42",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let expected = add_message(
                    &committed,
                    "one",
                    "Second queued words.",
                    "2026-10-30 07:43",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(&root, &[edit], "first queued save");
                assert!(
                    outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                    "{} {:?}",
                    outcome.error,
                    outcome.notes
                );
                assert_eq!(f.main("docs/a.md"), committed);
                assert_eq!(
                    std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                    expected
                );
                let edit = Edit {
                    rel: "docs/a.md".into(),
                    base: saved,
                    new: queued,
                };
                let outcome = commit_verdicts(&root, &[edit], "second queued save");
                assert!(
                    outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                    "{} {:?}",
                    outcome.error,
                    outcome.notes
                );
                assert_eq!(f.main("docs/a.md"), expected);
                assert_eq!(
                    std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                    expected
                );
                assert!(git(&root, &["status", "--porcelain"]).is_empty());
            }
            f.clean_workers();
        },
    );
}

#[test]
fn generated_stale_verdict_keeps_the_concurrent_reply_in_the_checkout() {
    isolated(
        "generated_stale_verdict_keeps_the_concurrent_reply_in_the_checkout",
        |f| {
            use symphony::edit::{add_message, set_decision};
            use symphony::register::Who;
            let root = f.repo();
            f.generate();
            let current = add_message(
                DOC,
                "one",
                "Concurrent reply.",
                "2026-10-30 07:41",
                Who::Agent,
                None,
            )
            .unwrap();
            write(&root.join("docs/a.md"), &current);
            git(
                &root,
                &["commit", "-qam", "main reply before generated verdict"],
            );
            let saved = set_decision(
                DOC,
                "one",
                "approved",
                "Owner's exact words.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            let expected = set_decision(
                &current,
                "one",
                "approved",
                "Owner's exact words.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            let committer = ClosingCommitter::start(&root);
            committer
                .0
                .submit(f.edit("docs/a.md", DOC, &saved))
                .unwrap();
            committer.0.close();
            assert_eq!(f.main("docs/a.md"), expected);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                expected
            );
            assert_eq!(f.main("content/one"), "approved\n");
            assert_eq!(f.main("view/lamp_atlas"), "approved\n");
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            assert!(
                f.store().tasks(None).unwrap().is_empty(),
                "successful output publication must not file a failure task"
            );
            f.clean_workers();
        },
    );
}

#[test]
fn syncing_a_stale_save_keeps_later_local_edits_and_queued_messages() {
    isolated(
        "syncing_a_stale_save_keeps_later_local_edits_and_queued_messages",
        |f| {
            use symphony::edit::{add_message, set_decision};
            use symphony::register::{Who, sort_register};
            let root = f.repo();
            let base = sort_register("# Prose\n\n<!-- register:start -->\n### Topic\n\n- [ ] **c** — Existing card.\n<!-- register:end -->\n", &common::plain()).unwrap();
            write(&root.join("docs/a.md"), &base);
            git(&root, &["commit", "-qam", "base card"]);
            let current =
                add_message(&base, "c", "Reply.", "2026-10-30 07:41", Who::Agent, None).unwrap();
            write(&root.join("docs/a.md"), &current);
            git(&root, &["commit", "-qam", "main reply"]);
            let saved = set_decision(
                &base,
                "c",
                "approved",
                "Keep it.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            let edit = f.edit("docs/a.md", &base, &saved);
            let queued = add_message(
                &saved,
                "c",
                "Later queued words.",
                "2026-10-30 07:43",
                Who::Owner,
                None,
            )
            .unwrap();
            write(
                &root.join("docs/a.md"),
                &queued.replace("# Prose", "# Local prose"),
            );
            let outcome = commit_verdicts(&root, &[edit], "first save");
            assert!(
                outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                "{} {:?}",
                outcome.error,
                outcome.notes
            );
            let committed = set_decision(
                &current,
                "c",
                "approved",
                "Keep it.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            assert_eq!(f.main("docs/a.md"), committed);
            let expected = add_message(
                &committed,
                "c",
                "Later queued words.",
                "2026-10-30 07:43",
                Who::Owner,
                None,
            )
            .unwrap();
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                expected.replace("# Prose", "# Local prose")
            );
            let edit = Edit {
                rel: "docs/a.md".into(),
                base: saved,
                new: queued,
            };
            let outcome = commit_verdicts(&root, &[edit], "second save");
            assert!(
                outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                "{} {:?}",
                outcome.error,
                outcome.notes
            );
            assert_eq!(f.main("docs/a.md"), expected);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                expected.replace("# Prose", "# Local prose")
            );
            f.clean_workers();
        },
    );
}

#[test]
fn stale_saves_merge_multiple_registers() {
    isolated("stale_saves_merge_multiple_registers", |f| {
        stale_save_shapes(f, StaleSaveShape::Registers)
    });
}

#[test]
fn stale_saves_keep_blank_lines_inside_posts() {
    isolated("stale_saves_keep_blank_lines_inside_posts", |f| {
        stale_save_shapes(f, StaleSaveShape::Multiline)
    });
}

#[test]
fn stale_saves_keep_a_new_card_after_the_edited_card() {
    isolated("stale_saves_keep_a_new_card_after_the_edited_card", |f| {
        stale_save_shapes(f, StaleSaveShape::CardAfter)
    });
}

#[test]
fn stale_saves_keep_a_new_card_before_the_edited_card() {
    isolated("stale_saves_keep_a_new_card_before_the_edited_card", |f| {
        stale_save_shapes(f, StaleSaveShape::CardBefore)
    });
}

enum StaleSaveShape {
    Registers,
    Multiline,
    CardAfter,
    CardBefore,
}

fn stale_save_shapes(f: &Fixture, shape: StaleSaveShape) {
    use symphony::edit::{add_message, set_decision};
    use symphony::register::{Who, sort_register};
    let root = f.repo();
    let card = "- [ ] **c** — Existing card.\n  [2026-10-30 07:00] agent — First paragraph.\n\n    Second paragraph stays verbatim.\n";
    let region = |slug: &str, body: &str| {
        format!(
            "## Section {slug}\n\n<!-- register:start -->\n### Topic {slug}\n\n{body}\n<!-- register:end -->\n"
        )
    };
    let multiple = (0..6)
        .map(|i| region(&i.to_string(), &card.replace("**c**", &format!("**c{i}**"))))
        .collect::<String>();
    let single = region("one", card);
    let cases = match shape {
        StaleSaveShape::Registers => [(multiple.as_str(), "c0", 0), (multiple.as_str(), "c5", 0)],
        StaleSaveShape::Multiline => [(single.as_str(), "c", 0), (card, "c", 0)],
        StaleSaveShape::CardAfter => [(single.as_str(), "c", 1), (card, "c", 1)],
        StaleSaveShape::CardBefore => [(single.as_str(), "c", -1), (card, "c", -1)],
    };
    for (base, slug, adjacent) in cases {
        for verdict in [false, true] {
            let base = sort_register(base, &common::plain()).unwrap();
            let mut current = add_message(
                &base,
                slug,
                "Concurrent agent reply.",
                "2026-10-30 07:42",
                Who::Agent,
                None,
            )
            .unwrap();
            if adjacent != 0 {
                let added = "- [ ] **fresh** — Independent new card.\n    Proposal details stay verbatim.\n  [2026-10-30 07:41] agent — New card history.\n";
                current = if adjacent < 0 {
                    sort_register(
                        &current.replace("- [ ] **c**", &format!("{added}\n- [ ] **c**")),
                        &common::plain(),
                    )
                    .unwrap()
                } else if current.contains("<!-- register:end -->") {
                    sort_register(
                        &current.replace(
                            "<!-- register:end -->",
                            &format!("{added}\n<!-- register:end -->"),
                        ),
                        &common::plain(),
                    )
                    .unwrap()
                } else {
                    format!("{current}\n{added}")
                };
            }
            let saved = if verdict {
                set_decision(
                    &base,
                    slug,
                    "approved",
                    "Owner's exact words.\n\nAnother paragraph.",
                    "2026-10-30 07:43",
                    &common::plain(),
                )
                .unwrap()
            } else {
                add_message(
                    &base,
                    slug,
                    "Owner's exact words.\n\nAnother paragraph.",
                    "2026-10-30 07:43",
                    Who::Owner,
                    None,
                )
                .unwrap()
            };
            let expected = if verdict {
                set_decision(
                    &current,
                    slug,
                    "approved",
                    "Owner's exact words.\n\nAnother paragraph.",
                    "2026-10-30 07:43",
                    &common::plain(),
                )
                .unwrap()
            } else {
                add_message(
                    &current,
                    slug,
                    "Owner's exact words.\n\nAnother paragraph.",
                    "2026-10-30 07:43",
                    Who::Owner,
                    None,
                )
                .unwrap()
            };
            let saved = saved.replace(
                "Owner's exact words.\n    Another",
                "Owner's exact words.\n\n    Another",
            );
            let expected = expected.replace(
                "Owner's exact words.\n    Another",
                "Owner's exact words.\n\n    Another",
            );
            let expected = sort_register(&expected, &common::plain()).unwrap();
            write(&root.join("docs/a.md"), &base);
            git(&root, &["commit", "-qam", "base history"]);
            write(&root.join("docs/a.md"), &current);
            git(&root, &["commit", "-qam", "concurrent history"]);
            let edit = f.edit("docs/a.md", &base, &saved);
            let outcome = commit_verdicts(&root, &[edit], "owner save");
            assert!(
                outcome.committed && outcome.error.is_empty() && outcome.notes.is_empty(),
                "{} {:?}",
                outcome.error,
                outcome.notes
            );
            assert_eq!(f.main("docs/a.md"), expected);
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                expected
            );
            assert_eq!(
                git(&root, &["status", "--porcelain", "--", "docs/a.md"]),
                ""
            );

            let edited = current.replace("Existing card.", "Edited in place.");
            write(&root.join("docs/a.md"), &edited);
            git(&root, &["commit", "-qam", "changed card body"]);
            let tip = git(&root, &["rev-parse", "main"]);
            let refused = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &base, &saved)],
                "conflicting owner save",
            );
            assert!(
                !refused.committed && refused.error.contains("docs/a.md conflicts with main"),
                "{}",
                refused.error
            );
            assert_eq!(git(&root, &["rev-parse", "main"]), tip);
            assert_eq!(f.main("docs/a.md"), edited);
        }
    }
    f.clean_workers();
}

#[test]
fn a_card_added_on_main_survives_a_stale_message_in_plain_and_register_documents() {
    isolated(
        "a_card_added_on_main_survives_a_stale_message_in_plain_and_register_documents",
        |f| {
            use symphony::edit::add_message;
            use symphony::register::{Who, sort_register};

            let root = f.repo();
            let plain = "- [ ] **c** — Existing card.\n\n- [ ] **other** — Another card.\n";
            let register = sort_register(&format!("# Decisions\n\n<!-- register:start -->\n### Topic\n\n{plain}<!-- register:end -->\n"), &common::plain()).unwrap();
            for base in [plain, register.as_str()] {
                let added = "- [ ] **fresh** — Added independently on main.\n";
                let current = if base.contains("<!-- register:end -->") {
                    sort_register(
                        &base.replace(
                            "<!-- register:end -->",
                            &format!("{added}<!-- register:end -->"),
                        ),
                        &common::plain(),
                    )
                    .unwrap()
                } else {
                    format!("{base}\n{added}")
                };
                let saved = add_message(
                    base,
                    "c",
                    "Owner's stale message.",
                    "2026-10-30 07:42",
                    Who::Owner,
                    None,
                )
                .unwrap();
                write(&root.join("docs/a.md"), base);
                git(&root, &["commit", "-qam", "base document"]);
                write(&root.join("docs/a.md"), &current);
                git(&root, &["commit", "-qam", "main adds another card"]);
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", base, &saved)],
                    "stale owner message",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                assert_eq!(
                    f.main("docs/a.md"),
                    add_message(
                        &current,
                        "c",
                        "Owner's stale message.",
                        "2026-10-30 07:42",
                        Who::Owner,
                        None
                    )
                    .unwrap()
                );
            }
            f.clean_workers();
        },
    );
}

#[test]
fn a_clean_text_merge_cannot_resurrect_a_card_removed_on_main() {
    isolated(
        "a_clean_text_merge_cannot_resurrect_a_card_removed_on_main",
        |f| {
            use symphony::edit::set_decision;
            use symphony::register::{parse_decisions, sort_register};

            let root = f.repo();
            let removed = "- [?] **c** — Choose a framework.\n  [2026-10-30 07:00] agent · question — Which?\n";
            let base = sort_register(&format!("# Decisions\n\n<!-- register:start -->\n### Topic\n\n{removed}\n- [ ] **pending** — Still pending.\n\n- [?] **answered** — Already answered.\n  [2026-10-30 07:00] owner · question — Explain.\n  [2026-10-30 07:01] agent — Explained.\n\n- [?] **waiting** — Still waiting.\n  [2026-10-30 07:00] owner · question — Explain this too.\n<!-- register:end -->\n"), &common::plain()).unwrap();
            assert!(base.contains(removed));
            let current = sort_register(&base.replace(removed, ""), &common::plain()).unwrap();
            let saved = set_decision(
                &base,
                "c",
                "approved",
                "Keep Dioxus.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            for (name, text) in [
                ("merge-current", &current),
                ("merge-base", &base),
                ("merge-save", &saved),
            ] {
                write(&root.join(name), text);
            }
            let raw = sys::output(
                sys::Command::new("git")
                    .args([
                        "merge-file",
                        "-p",
                        "merge-current",
                        "merge-base",
                        "merge-save",
                    ])
                    .current_dir(&root),
            )
            .unwrap();
            assert!(
                raw.status.success(),
                "fixture must reach the clean merge path: {}",
                String::from_utf8_lossy(&raw.stdout)
            );
            let raw = String::from_utf8(raw.stdout).unwrap();
            assert_eq!(
                parse_decisions(&raw.lines().collect::<Vec<_>>())
                    .iter()
                    .filter(|card| card.slug == "c")
                    .count(),
                1
            );
            write(&root.join("docs/a.md"), &base);
            git(&root, &["commit", "-qam", "base register"]);
            write(&root.join("docs/a.md"), &current);
            git(&root, &["commit", "-qam", "main removes the card"]);
            let tip = git(&root, &["rev-parse", "main"]);
            let outcome = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &base, &saved)],
                "stale verdict on removed card",
            );
            assert!(
                !outcome.committed && outcome.error.contains("docs/a.md conflicts with main"),
                "{}",
                outcome.error
            );
            assert_eq!(git(&root, &["rev-parse", "main"]), tip);
            assert_eq!(f.main("docs/a.md"), current);
            f.clean_workers();
        },
    );
}

#[test]
fn clean_text_merge_of_two_group_moves_keeps_one_card_and_accepts_the_next_save() {
    isolated(
        "clean_text_merge_of_two_group_moves_keeps_one_card_and_accepts_the_next_save",
        |f| {
            use symphony::edit::{add_message, set_decision};
            use symphony::register::{Who, parse_decisions, sort_register};

            let root = f.repo();
            let base = sort_register("# Decisions\n\n<!-- register:start -->\n### Topic\n\n- [?] **c** — Choose a framework.\n  [2026-10-30 07:00] agent · question — Which?\n\n- [ ] **pending** — Still pending.\n\n- [?] **answered** — Already answered.\n  [2026-10-30 07:00] owner · question — Explain.\n  [2026-10-30 07:01] agent — Explained.\n\n- [?] **waiting** — Still waiting.\n  [2026-10-30 07:00] owner · question — Explain this too.\n<!-- register:end -->\n", &common::plain()).unwrap();
            let question = add_message(
                &base,
                "c",
                "Can we keep Dioxus?",
                "2026-10-30 07:37",
                Who::Owner,
                Some("question"),
            )
            .unwrap();
            let current = sort_register(
                &add_message(
                    &question,
                    "c",
                    "Yes, with tokens.",
                    "2026-10-30 07:38",
                    Who::Agent,
                    None,
                )
                .unwrap(),
                &common::plain(),
            )
            .unwrap();
            let saved = set_decision(
                &base,
                "c",
                "approved",
                "Keep Dioxus.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            for (name, text) in [
                ("merge-current", &current),
                ("merge-base", &base),
                ("merge-save", &saved),
            ] {
                write(&root.join(name), text);
            }
            let plain = sys::output(
                sys::Command::new("git")
                    .args([
                        "merge-file",
                        "-p",
                        "merge-current",
                        "merge-base",
                        "merge-save",
                    ])
                    .current_dir(&root),
            )
            .unwrap();
            assert!(
                plain.status.success(),
                "fixture must reach the clean merge path: {}",
                String::from_utf8_lossy(&plain.stdout)
            );
            let plain = String::from_utf8(plain.stdout).unwrap();
            assert_eq!(
                parse_decisions(&plain.lines().collect::<Vec<_>>())
                    .iter()
                    .filter(|card| card.slug == "c")
                    .count(),
                2
            );

            write(&root.join("docs/a.md"), &base);
            git(&root, &["commit", "-qam", "base register"]);
            write(&root.join("docs/a.md"), &current);
            git(
                &root,
                &["commit", "-qam", "main moves the card to answered"],
            );
            let outcome = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &base, &saved)],
                "stale owner verdict",
            );
            assert!(
                outcome.committed && outcome.error.is_empty(),
                "{}",
                outcome.error
            );
            let expected = set_decision(
                &current,
                "c",
                "approved",
                "Keep Dioxus.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            assert_eq!(f.main("docs/a.md"), expected);
            let next = set_decision(
                &expected,
                "waiting",
                "approved",
                "Now answered.",
                "2026-10-30 07:43",
                &common::plain(),
            )
            .unwrap();
            let outcome = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &expected, &next)],
                "next verdict on another card",
            );
            assert!(
                outcome.committed && outcome.error.is_empty(),
                "{}",
                outcome.error
            );
            assert_eq!(f.main("docs/a.md"), next);
            // A clean duplicated merge must still refuse a changed card body when the history
            // fallback cannot resolve it, rather than publishing either copy.
            let edited = current.replace("Choose a framework.", "Changed framework question.");
            write(&root.join("merge-current"), &edited);
            let plain = sys::output(
                sys::Command::new("git")
                    .args([
                        "merge-file",
                        "-p",
                        "merge-current",
                        "merge-base",
                        "merge-save",
                    ])
                    .current_dir(&root),
            )
            .unwrap();
            assert!(
                plain.status.success(),
                "edited-card fixture must reach the clean merge path"
            );
            write(&root.join("docs/a.md"), &edited);
            git(
                &root,
                &["commit", "-qam", "main edits and regroups the card"],
            );
            let tip = git(&root, &["rev-parse", "main"]);
            let refused = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &base, &saved)],
                "stale verdict on edited card",
            );
            assert!(
                !refused.committed && refused.error.contains("docs/a.md conflicts with main"),
                "{}",
                refused.error
            );
            assert_eq!(git(&root, &["rev-parse", "main"]), tip);
            assert_eq!(f.main("docs/a.md"), edited);
            f.clean_workers();
        },
    );
}

#[test]
fn stale_owner_message_preserves_quoted_entries_and_out_of_order_saved_lines() {
    isolated(
        "stale_owner_message_preserves_quoted_entries_and_out_of_order_saved_lines",
        |f| {
            use symphony::edit::add_message;
            use symphony::register::Who;

            let root = f.repo();
            let base = "- [ ] **c** — A card.\n";
            let reply =
                add_message(base, "c", "Reply.", "2026-10-30 07:45", Who::Agent, None).unwrap();
            let quoted = add_message(
                base,
                "c",
                "My words:\n[2026-10-30 06:00] agent — Quoted line.\nEnd of my words.",
                "2026-10-30 07:50",
                Who::Owner,
                None,
            )
            .unwrap();
            let owner_lines = "  [2026-10-30 07:50] owner — My words:\n    [2026-10-30 06:00] agent — Quoted line.\n    End of my words.\n";
            let out_of_order = format!(
                "{base}  [2026-10-30 07:50] owner — First.\n  [2026-10-30 06:00] owner — Second.\n"
            );
            for (saved, additions) in [
                (&quoted, owner_lines),
                (&out_of_order, out_of_order.strip_prefix(base).unwrap()),
            ] {
                write(&root.join("docs/a.md"), &reply);
                git(
                    &root,
                    &["commit", "-qam", "main reply before stale message"],
                );
                let outcome =
                    commit_verdicts(&root, &[f.edit("docs/a.md", base, saved)], "stale message");
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                assert_eq!(f.main("docs/a.md"), format!("{reply}{additions}"));
            }
            f.clean_workers();
        },
    );
}

#[test]
fn register_verdicts_merge_generated_groups_without_reordering_existing_history() {
    isolated(
        "register_verdicts_merge_generated_groups_without_reordering_existing_history",
        |f| {
            use symphony::edit::{add_message, set_decision};
            use symphony::register::{Who, sort_register};

            let root = f.repo();
            let first = "  [2026-10-30 07:30] owner — First, kept before second.\n";
            let second = "  [2026-10-30 07:20] owner — Second, kept after first.\n";
            let other_history = "  [2026-10-30 07:50] owner — Other first.\n  [2026-10-30 07:40] owner — Other second.\n";
            for mark in [" ", "x", "?"] {
                let base = sort_register(&format!("# Decision\n\nProse.\n\n<!-- register:start -->\n### Framework\n\n- [{mark}] **choice** — Choose.\n{first}{second}  [2026-10-30 07:37] owner · question — Can we keep Dioxus with themes?\n\n- [ ] **other** — Another.\n{other_history}<!-- register:end -->\n\nTail.\n"), &common::plain()).unwrap();
                write(&root.join("docs/a.md"), &base);
                git(&root, &["commit", "-qam", "register fixture"]);
                let replied = sort_register(
                    &add_message(
                        &base,
                        "choice",
                        "Use tokens.\nKeep continuation.",
                        "2026-10-30 07:38",
                        Who::Agent,
                        None,
                    )
                    .unwrap(),
                    &common::plain(),
                )
                .unwrap();
                let saved = set_decision(
                    &base,
                    "choice",
                    "approved",
                    "Keep Dioxus.",
                    "2026-10-30 07:42",
                    &common::plain(),
                )
                .unwrap();
                assert_ne!(replied, saved);
                if mark == "?" {
                    assert!(replied.contains("## Answered: approve again or comment"));
                    assert!(saved.contains("## Waiting on an agent"));
                }
                write(&root.join("docs/a.md"), &replied);
                git(&root, &["commit", "-qam", "agent reply regroups register"]);
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &base, &saved)],
                    "owner approval",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                let expected = set_decision(
                    &replied,
                    "choice",
                    "approved",
                    "Keep Dioxus.",
                    "2026-10-30 07:42",
                    &common::plain(),
                )
                .unwrap();
                assert_eq!(f.main("docs/a.md"), expected);
                assert!(expected.contains(&format!("{first}{second}")));
                assert!(expected.contains(other_history));
                // A second save can still be based on the first save before it saw the agent reply.
                let next = set_decision(
                    &saved,
                    "choice",
                    "approved",
                    "Also compare repeat visits.",
                    "2026-10-30 07:44",
                    &common::plain(),
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &saved, &next)],
                    "second owner save",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                assert_eq!(
                    f.main("docs/a.md"),
                    set_decision(
                        &expected,
                        "choice",
                        "approved",
                        "Also compare repeat visits.",
                        "2026-10-30 07:44",
                        &common::plain()
                    )
                    .unwrap()
                );
            }
            f.clean_workers();
        },
    );
}

#[test]
fn register_regrouping_does_not_allow_removed_edited_or_extracted_cards() {
    isolated(
        "register_regrouping_does_not_allow_removed_edited_or_extracted_cards",
        |f| {
            use symphony::edit::{add_message, set_decision};
            use symphony::register::{Who, sort_register};

            let root = f.repo();
            let block = "- [ ] **choice** — Choose.\n  [2026-10-30 07:30] owner — Original.\n";
            let base = sort_register(&format!("# Decision\n\nProse.\n\n<!-- register:start -->\n### Topic\n\n{block}\n- [ ] **other** — Other.\n<!-- register:end -->\n"), &common::plain()).unwrap();
            let saved = set_decision(
                &base,
                "choice",
                "approved",
                "Yes.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            let replied = sort_register(
                &add_message(
                    &base,
                    "choice",
                    "Reply.",
                    "2026-10-30 07:38",
                    Who::Agent,
                    None,
                )
                .unwrap(),
                &common::plain(),
            )
            .unwrap();
            let replied_block = format!("{block}  [2026-10-30 07:38] agent — Reply.\n");
            for main_text in [
                replied.replace("Choose.", "Changed card."),
                replied.replace("Original.", "Edited owner line."),
                replied.replace("Prose.", "Changed prose."),
                replied.replace(&replied_block, ""),
                replied.replace(&replied_block, "") + "\n" + &replied_block,
                replied.replace("### Topic", "### Different topic"),
                replied.replace(
                    "<!-- register:start -->",
                    "<!-- register:start -->\n    Unowned prose.",
                ),
            ] {
                write(&root.join("docs/a.md"), &main_text);
                git(&root, &["commit", "-qam", "main changes register content"]);
                let tip = git(&root, &["rev-parse", "main"]);
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &base, &saved)],
                    "stale owner save",
                );
                assert!(
                    !outcome.committed && outcome.error.contains("docs/a.md conflicts with main"),
                    "main: {main_text:?}; {}",
                    outcome.error
                );
                assert_eq!(git(&root, &["rev-parse", "main"]), tip);
                assert_eq!(f.main("docs/a.md"), main_text);
            }
            f.clean_workers();
        },
    );
}

#[test]
fn unterminated_history_entry_is_separated_from_a_stale_owner_save() {
    isolated(
        "unterminated_history_entry_is_separated_from_a_stale_owner_save",
        |f| {
            let root = f.repo();
            let base = "- [ ] **choice** — Choose.\n";
            let replied = format!("{base}  [2026-10-30 07:38] agent — Reply.");
            let saved = symphony::edit::set_decision(
                base,
                "choice",
                "approved",
                "Keep Dioxus.",
                "2026-10-30 07:42",
                &common::plain(),
            )
            .unwrap();
            write(&root.join("docs/a.md"), base);
            git(&root, &["commit", "-qam", "unterminated history base"]);
            write(&root.join("docs/a.md"), &replied);
            git(&root, &["commit", "-qam", "unterminated agent reply"]);
            let outcome =
                commit_verdicts(&root, &[f.edit("docs/a.md", base, &saved)], "owner save");
            assert!(
                outcome.committed && outcome.error.is_empty(),
                "{}",
                outcome.error
            );
            assert_eq!(
                f.main("docs/a.md"),
                "- [x] **choice** — Choose.\n  [2026-10-30 07:38] agent — Reply.\n  [2026-10-30 07:42] owner · approved — Keep Dioxus.\n"
            );
            f.clean_workers();
        },
    );
}

#[test]
fn stale_owner_comment_stays_open_until_a_reply_sees_it() {
    isolated(
        "stale_owner_comment_stays_open_until_a_reply_sees_it",
        |f| {
            use symphony::edit::add_message;
            use symphony::register::{Who, parse_decisions, sort_register};

            let root = f.repo();
            let status = || {
                stage("reading open-card status");
                write(&root.join("docs/a.md"), &f.main("docs/a.md"));
                let status = common::status_ok(&root, &["--open"]);
                stage("open-card status read");
                status
            };
            for register in [false, true] {
                let base = "- [ ] **choice** — Choose.\n  [2026-10-30 06:00] owner — Previously seen comment.\n";
                let base = if register {
                    sort_register(
                        &format!("<!-- register:start -->\n{base}<!-- register:end -->\n"),
                        &common::plain(),
                    )
                    .unwrap()
                } else {
                    base.into()
                };
                write(&root.join("docs/a.md"), &base);
                git(&root, &["commit", "-qam", "card fixture"]);
                let replied = add_message(
                    &base,
                    "choice",
                    "Existing reply.",
                    "2026-10-30 08:00",
                    Who::Agent,
                    None,
                )
                .unwrap();
                let replied = add_message(
                    &replied,
                    "choice",
                    "A question from the agent.",
                    "2026-10-30 08:01",
                    Who::Agent,
                    Some("question"),
                )
                .unwrap();
                write(&root.join("docs/a.md"), &replied);
                git(&root, &["commit", "-qam", "agent reply"]);
                let saved = add_message(
                    &base,
                    "choice",
                    "Unseen comment.\nSecond paragraph.",
                    "2026-10-30 07:00",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &base, &saved)],
                    "owner comment",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                let merged = f.main("docs/a.md");
                assert!(merged.contains("  [2026-10-30 07:00] owner — Unseen comment.\n    Second paragraph.\n  [2026-10-30 08:00] agent — Existing reply.\n"), "{merged}");
                let card = parse_decisions(&merged.lines().collect::<Vec<_>>()).remove(0);
                assert_eq!(card.entries().len(), 4, "metadata is not a post");
                assert_eq!(card.unanswered(), ["Unseen comment.\nSecond paragraph."]);
                assert!(card.open(&common::plain()));
                assert_eq!(
                    card.group(&common::plain()),
                    symphony::register::Group::Decide
                );
                assert!(
                    symphony::verdicts::open_cards(&root)
                        .unwrap()
                        .iter()
                        .any(|card| card.slug == "choice")
                );
                assert!(status().contains("Unseen comment."));
                assert_eq!(sort_register(&merged, &common::plain()).unwrap(), merged);

                // A second queued owner save still uses a base without main's reply or metadata.
                let queued = add_message(
                    &saved,
                    "choice",
                    "Another unseen comment.",
                    "2026-10-30 07:30",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &saved, &queued)],
                    "queued owner comment",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                let merged = f.main("docs/a.md");
                let card = parse_decisions(&merged.lines().collect::<Vec<_>>()).remove(0);
                assert_eq!(
                    card.unanswered(),
                    [
                        "Unseen comment.\nSecond paragraph.",
                        "Another unseen comment."
                    ]
                );

                // Even an identical reply appended in the same minute is a new occurrence.
                let answered = add_message(
                    &merged,
                    "choice",
                    "Existing reply.",
                    "2026-10-30 08:00",
                    Who::Agent,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &merged, &answered)],
                    "agent answer",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                let answered = f.main("docs/a.md");
                let card = parse_decisions(&answered.lines().collect::<Vec<_>>()).remove(0);
                assert!(!card.open(&common::plain()));
                assert!(card.unanswered().is_empty());
                assert!(!status().contains("Unseen comment."));

                // A queued save arriving after that answer opens only its newly merged comment.
                let late = add_message(
                    &queued,
                    "choice",
                    "Late unseen comment.",
                    "2026-10-30 07:45",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &queued, &late)],
                    "late queued owner comment",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                let answered = f.main("docs/a.md");
                let card = parse_decisions(&answered.lines().collect::<Vec<_>>()).remove(0);
                assert_eq!(card.unanswered(), ["Late unseen comment."]);

                // An ordinary, current save continues to use the page's reply order.
                let comment = add_message(
                    &answered,
                    "choice",
                    "Seen comment.",
                    "2026-10-30 09:00",
                    Who::Owner,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &answered, &comment)],
                    "current owner comment",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                assert!(status().contains("Seen comment."));
                let reply = add_message(
                    &comment,
                    "choice",
                    "Answered.",
                    "2026-10-30 09:01",
                    Who::Agent,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &comment, &reply)],
                    "current agent answer",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                assert!(!status().contains("Seen comment."));
                assert!(!status().contains("Late unseen comment."));
            }
            f.clean_workers();
        },
    );
}

#[test]
fn stale_owner_verdict_without_words_stays_open_until_answered() {
    isolated(
        "stale_owner_verdict_without_words_stays_open_until_answered",
        |f| {
            use symphony::edit::{add_message, set_decision};
            use symphony::register::{Who, parse_decisions};

            let root = f.repo();
            for verdict in ["rejected", "question", "approved"] {
                let base = "- [ ] **choice** — Choose.\n";
                write(&root.join("docs/a.md"), base);
                git(&root, &["commit", "-qam", "card fixture"]);
                let replied = add_message(
                    base,
                    "choice",
                    "Existing reply.",
                    "2026-10-30 08:00",
                    Who::Agent,
                    None,
                )
                .unwrap();
                write(&root.join("docs/a.md"), &replied);
                git(&root, &["commit", "-qam", "agent reply"]);
                let saved = set_decision(
                    base,
                    "choice",
                    verdict,
                    "",
                    "2026-10-30 07:00",
                    &common::plain(),
                )
                .unwrap();
                let outcome =
                    commit_verdicts(&root, &[f.edit("docs/a.md", base, &saved)], "owner verdict");
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                let merged = f.main("docs/a.md");
                let card = parse_decisions(&merged.lines().collect::<Vec<_>>()).remove(0);
                assert!(card.open(&common::plain()), "{verdict}: {merged}");
                assert!(!card.resolvable(&common::plain()));
                assert!(card.unanswered().is_empty(), "no invented owner words");
                write(&root.join("docs/a.md"), &merged);
                let status = common::status_ok(&root, &["--open"]);
                assert!(status.contains("Choose."), "{verdict}: {status}");
                assert!(!status.contains("review:unseen-owner"), "{status}");
                let answered = add_message(
                    &merged,
                    "choice",
                    "Applied.",
                    "2026-10-30 08:01",
                    Who::Agent,
                    None,
                )
                .unwrap();
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", &merged, &answered)],
                    "agent answer",
                );
                assert!(
                    outcome.committed && outcome.error.is_empty(),
                    "{}",
                    outcome.error
                );
                let answered = f.main("docs/a.md");
                assert!(
                    !parse_decisions(&answered.lines().collect::<Vec<_>>())[0]
                        .open(&common::plain())
                );
            }
            f.clean_workers();
        },
    );
}

#[test]
fn stale_card_saves_keep_interleaved_history_in_timestamp_order() {
    isolated(
        "stale_card_saves_keep_interleaved_history_in_timestamp_order",
        |f| {
            let root = f.repo();
            let base = "# Decision\n\n- [ ] **choice** — Choose.\n\n- [ ] **other** — Another.\n";
            let first = "  [2026-10-30 07:40] owner · question — What about themes?\n";
            let reply = "  [2026-10-30 07:38] agent — Use tokens.\n    Keep this continuation.\n";
            let second = "  [2026-10-30 07:42] owner · approved — Keep Dioxus.\n";
            write(&root.join("docs/a.md"), base);
            git(&root, &["commit", "-qam", "card fixture"]);
            let saved = base.replace("Choose.\n", &format!("Choose.\n{first}"));
            let replied = base.replace("Choose.\n", &format!("Choose.\n{reply}"));
            write(&root.join("docs/a.md"), &replied);
            git(&root, &["commit", "-qam", "agent reply"]);
            let outcome = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", base, &saved)],
                "owner question",
            );
            assert!(
                outcome.committed && outcome.error.is_empty(),
                "{}",
                outcome.error
            );
            let together = base.replace("Choose.\n", &format!("Choose.\n{reply}{first}"));
            assert_eq!(f.main("docs/a.md"), together);
            // The next save was written against the first save, before its merge saw the reply.
            let approved = saved
                .replace("[ ] **choice**", "[x] **choice**")
                .replace(first, &format!("{first}{second}"));
            let outcome = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &saved, &approved)],
                "owner approval",
            );
            assert!(
                outcome.committed && outcome.error.is_empty(),
                "{}",
                outcome.error
            );
            let expected = together
                .replace("[ ] **choice**", "[x] **choice**")
                .replace(first, &format!("{first}{second}"));
            assert_eq!(f.main("docs/a.md"), expected);
            // Independent histories in the same file still use the ordinary merge path.
            let other = together.replace(
                "Another.\n",
                "Another.\n  [2026-10-30 07:43] agent — Another reply.\n",
            );
            let outcome = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &together, &other)],
                "other reply",
            );
            assert!(
                outcome.committed && outcome.error.is_empty(),
                "{}",
                outcome.error
            );
            assert_eq!(
                f.main("docs/a.md"),
                expected.replace(
                    "Another.\n",
                    "Another.\n  [2026-10-30 07:43] agent — Another reply.\n"
                )
            );
            f.clean_workers();
        },
    );
}

#[test]
fn stale_card_append_refuses_changes_to_existing_content() {
    isolated(
        "stale_card_append_refuses_changes_to_existing_content",
        |f| {
            let root = f.repo();
            let base = "# Decision\n\nProse.\n\n- [ ] **choice** — Choose.\n  [2026-10-30 07:37] owner · question — Original.\n\n- [ ] **other** — Another.\n";
            let save = base.replace(
                "Original.\n",
                "Original.\n  [2026-10-30 07:42] owner · approved — Keep Dioxus.\n",
            );
            for changed in [
                base.replace("Original.", "Edited."),
                base.replace("Choose.", "Changed card."),
                base.replace("Prose.", "Changed prose."),
                base.replace("- [ ] **choice** — Choose.\n", ""),
                base.replace("- [ ] **choice** — Choose.\n  [2026-10-30 07:37] owner · question — Original.\n\n", "")
                    + "\n- [ ] **choice** — Choose.\n  [2026-10-30 07:37] owner · question — Original.\n  [2026-10-30 07:38] agent — Reply after moving.\n",
            ] {
                // Both sides append at the same position, so the fallback must validate the
                // unchanged content rather than accepting only the conflicting history hunk.
                let changed = changed.replace(
                    "\n\n- [ ] **other**",
                    "\n  [2026-10-30 07:38] agent — Reply.\n\n- [ ] **other**",
                );
                write(&root.join("docs/a.md"), &changed);
                git(&root, &["commit", "-qam", "main changes existing content"]);
                let tip = git(&root, &["rev-parse", "main"]);
                let outcome = commit_verdicts(
                    &root,
                    &[f.edit("docs/a.md", base, &save)],
                    "stale owner save",
                );
                assert!(
                    !outcome.committed && outcome.error.contains("docs/a.md conflicts with main"),
                    "main content: {changed:?}; {}",
                    outcome.error
                );
                assert_eq!(git(&root, &["rev-parse", "main"]), tip);
                assert_eq!(f.main("docs/a.md"), changed);
            }
            f.clean_workers();
        },
    );
}

#[test]
fn invalid_conflicting_and_guard_refused_edits_leave_main_unchanged() {
    isolated(
        "invalid_conflicting_and_guard_refused_edits_leave_main_unchanged",
        |f| {
            let root = f.repo();
            let before = git(&root, &["rev-parse", "main"]);
            for rel in [
                "other",
                "docs/data/x.json",
                "../outside.md",
                "docs/.hidden.md",
                "docs/missing.md",
            ] {
                let outcome = commit_verdicts(
                    &root,
                    &[Edit {
                        rel: rel.into(),
                        base: DOC.into(),
                        new: "changed\n".into(),
                    }],
                    "refused",
                );
                assert!(
                    !outcome.committed && outcome.error.contains(rel),
                    "{rel}: {}",
                    outcome.error
                );
                assert_eq!(git(&root, &["rev-parse", "main"]), before);
            }
            let changed = DOC.replace("proposal.", "changed on main.");
            write(&root.join("docs/a.md"), &changed);
            git(&root, &["commit", "-qam", "changed main"]);
            let before = git(&root, &["rev-parse", "main"]);
            let conflict = commit_verdicts(
                &root,
                &[Edit {
                    rel: "docs/a.md".into(),
                    base: DOC.into(),
                    new: DOC.replace("proposal.", "different proposal."),
                }],
                "conflict",
            );
            assert!(
                !conflict.committed && conflict.error.contains("conflicts with main"),
                "{}",
                conflict.error
            );
            prepared_script(
                &root.join(".git/hooks/reference-transaction"),
                "echo 'fixture guard refuses main' >&2\nexit 1",
            );
            let refused = commit_verdicts(
                &root,
                &[f.edit("docs/a.md", &changed, &changed.replace("[ ]", "[x]"))],
                "guard",
            );
            assert!(
                !refused.committed && refused.error.contains("fixture guard refuses main"),
                "{}",
                refused.error
            );
            assert_eq!(git(&root, &["rev-parse", "main"]), before);
            f.clean_workers();
        },
    );
}

#[test]
fn saves_persist_verdicts_comments_asks_and_callbacks_across_restart() {
    isolated(
        "saves_persist_verdicts_comments_asks_and_callbacks_across_restart",
        |f| {
            let root = f.repo();
            let saves = ClosingSaves::start(&root);
            let decision = json!({"path":"docs/a.md", "slug":"one", "state":"approved", "note":"use this shape", "tap":"one-tap"});
            assert_eq!(
                saves.post("", "/api/decision", decision.clone()),
                json!({"ok":true})
            );
            assert_eq!(
                saves.post("", "/api/decision", decision.clone()),
                json!({"ok":true,"repeat":true,"state":"approved","note":"use this shape"})
            );
            // The tap sent again with other words writes nothing and answers with the words it
            // holds.
            let mut other = decision.clone();
            other["state"] = json!("rejected");
            other["note"] = json!("on second thought");
            assert_eq!(
                saves.post("", "/api/decision", other),
                json!({"ok":true,"repeat":true,"state":"approved","note":"use this shape"})
            );
            // The same answer under another tap, in the same minutes, is the same answer sent
            // again: nothing is written, and the answer names the words the card holds.
            let mut again = decision;
            again["tap"] = json!("another-tap");
            assert_eq!(
                saves.post("", "/api/decision", again),
                json!({"ok":true,"repeat":true,"state":"approved","note":"use this shape"})
            );
            saves.post("", "/api/comment", json!({"path":"docs/a.md", "after_line":1, "kind":"comment", "title":"Review detail", "body":"keep the outline"}));
            saves.post(
                "",
                "/api/message",
                json!({"path":"docs/a.md", "slug":"one", "text":"follow this up"}),
            );
            let id = saves.post("", "/api/ask", json!({"path":"docs/a.md", "question":"Why this lamp?", "anchor":"one", "quote":"proposal."}))["id"].as_i64().unwrap();
            saves.post(
                "",
                "/api/ask-reply",
                json!({"id":id, "text":"And its radius?"}),
            );
            // The close names each wait as it begins, the publications only once a save queued one.
            let said = std::cell::RefCell::new(Vec::new());
            saves
                .0
                .close_saying(|stage| said.borrow_mut().push(stage.to_owned()));
            assert_eq!(
                said.take(),
                ["the save in flight", "the queued verdict publications"]
            );
            let text = std::fs::read_to_string(root.join("docs/a.md")).unwrap();
            for words in [
                "[x] **one**",
                "use this shape",
                "Review detail",
                "keep the outline",
                "follow this up",
            ] {
                assert!(text.contains(words), "{words}: {text}");
            }
            assert_eq!(f.main("docs/a.md"), text);
            assert!(!text.contains("Why this lamp?"));
            let tip = git(&root, &["rev-parse", "main"]);
            let ask = symphony::asks::AskStore::open(&f.home.join("asks.db"))
                .unwrap()
                .thread(id)
                .unwrap();
            assert_eq!(
                (ask.path.as_str(), ask.anchor.as_str(), ask.quote.as_str()),
                ("docs/a.md", "one", "proposal.")
            );
            assert_eq!(
                ask.messages
                    .iter()
                    .map(|m| m.text.as_str())
                    .collect::<Vec<_>>(),
                ["Why this lamp?", "And its radius?"]
            );
            let restarted = ClosingSaves::start(&root);
            restarted
                .0
                .close_saying(|stage| said.borrow_mut().push(stage.to_owned()));
            assert_eq!(said.take(), ["the save in flight"]);
            assert_eq!(git(&root, &["rev-parse", "main"]), tip);
            let mut flusher = symphony::callbacks::Flusher::new(&root);
            flusher.flush(&root).unwrap();
            flusher.flush(&root).unwrap();
            let board = symphony_board::Database::named(&rpc::Client::new(
                &std::env::var("TASKS_SERVER").unwrap(),
                "sf",
                &std::env::var("TASKS_DATABASE").unwrap(),
            ))
            .unwrap();
            let rows = symphony_board::Raw::of(&board)
                .table("chat_event")
                .fields(&["text"])
                .unwrap();
            let events = rows.as_array().unwrap();
            assert_eq!(
                events.len(),
                5,
                "one callback per save, no repeated tap: {rows:?}"
            );
            let callbacks = rows.to_string();
            for words in [
                "use this shape",
                "keep the outline",
                "follow this up",
                "Why this lamp?",
                "And its radius?",
            ] {
                assert!(callbacks.contains(words), "{words}: {callbacks}");
            }
            // What waits on an agent is an ask, opened once with its record: the message joins its
            // card's, the follow-up its chat's. No save files a task.
            let store = f.store();
            let asks: Vec<_> = store
                .asks(None)
                .unwrap()
                .into_iter()
                .map(|ask| (ask.kind, ask.words, ask.lines))
                .collect();
            assert_eq!(
                asks,
                [
                    ("verdict".to_string(), "use this shape".to_string(), 2),
                    ("comment".into(), "Review detail keep the outline".into(), 1),
                    ("chat".into(), "Why this lamp?".into(), 2),
                ]
            );
            assert!(store.tasks(None).unwrap().is_empty(), "no task was filed");
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            f.clean_workers();
        },
    );
}

#[test]
fn quick_chat_acknowledges_durable_intents_before_delivery_and_replays_once() {
    isolated(
        "quick_chat_acknowledges_durable_intents_before_delivery_and_replays_once",
        |f| {
            let root = f.repo();
            let saves = ClosingSaves::start(&root);
            for words in ["First quick question", "Second quick question"] {
                let value = saves.post(
                    "",
                    "/api/quick-chat",
                    json!({"text":words,"page":"/docs/a.md#one"}),
                );
                assert!(value["id"].is_i64(), "{value}");
            }
            drop(saves);
            let store = f.store();
            assert!(store.asks(None).unwrap().is_empty(), "delivery has not run");
            let journal = f.home.join("asks.db");
            let sqlite = |sql: &str| {
                let done =
                    sys::output(sys::Command::new("sqlite3").arg(&journal).arg(sql)).unwrap();
                assert!(done.status.success(), "{done:?}");
                String::from_utf8_lossy(&done.stdout).trim().to_owned()
            };
            assert_eq!(sqlite("SELECT count(*) FROM callbacks;"), "2");
            assert_eq!(
                sqlite("SELECT count(*) FROM messages WHERE by = 'owner';"),
                "2"
            );
            // A crash after the board commits but before the local outbox deletes its record has
            // the same replay as this refused delete. The event key prevents a second delivery.
            sqlite(
                "CREATE TRIGGER hold_delivery BEFORE DELETE ON callbacks BEGIN SELECT RAISE(ABORT, 'held after board commit'); END;",
            );
            let error = symphony::callbacks::Flusher::new(&root)
                .flush(&root)
                .unwrap_err();
            assert!(error.contains("held after board commit"), "{error}");
            assert_eq!(sqlite("SELECT count(*) FROM callbacks;"), "2");
            sqlite("DROP TRIGGER hold_delivery;");
            let mut restarted = symphony::callbacks::Flusher::new(&root);
            restarted.flush(&root).unwrap();
            restarted.flush(&root).unwrap();
            assert_eq!(sqlite("SELECT count(*) FROM callbacks;"), "0");
            let asks = store.asks(None).unwrap();
            assert_eq!(asks.len(), 2);
            assert!(asks.iter().all(|ask| ask.lines == 1));
            let events = store.raw().table("chat_event").fields(&["text"]).unwrap();
            assert_eq!(events.as_array().unwrap().len(), 2);
            f.clean_workers();
        },
    );
}

/// A journal written before asks gains their column with its records kept, and a record whose ask
/// the board refuses still delivers its chat: neither holds the journal.
#[test]
fn a_journal_from_before_asks_and_an_ask_the_board_refuses_still_deliver_their_chat() {
    isolated(
        "a_journal_from_before_asks_and_an_ask_the_board_refuses_still_deliver_their_chat",
        |f| {
            let root = f.repo();
            let journal = f.home.join("asks.db");
            let sqlite = |sql: &str| {
                let done =
                    sys::output(sys::Command::new("sqlite3").arg(&journal).arg(sql)).unwrap();
                assert!(done.status.success(), "{done:?}");
                String::from_utf8_lossy(&done.stdout).trim().to_string()
            };
            sqlite(
                "CREATE TABLE callbacks (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL);
                 INSERT INTO callbacks(text) VALUES ('said before asks');",
            );
            let cardless = symphony_board::NewAsk {
                kind: "verdict".into(),
                path: Some("docs/a.md".into()),
                words: "too pale".into(),
                ..symphony_board::NewAsk::default()
            };
            symphony::callbacks::publish_asking(&root, "too pale", Some(&cardless)).unwrap();
            assert_eq!(
                sqlite("SELECT id, ask IS NULL FROM callbacks ORDER BY id;"),
                "1|1\n2|0"
            );
            symphony::callbacks::Flusher::new(&root)
                .flush(&root)
                .unwrap();
            assert_eq!(sqlite("SELECT count(*) FROM callbacks;"), "0");
            let board = symphony_board::Database::named(&rpc::Client::new(
                &std::env::var("TASKS_SERVER").unwrap(),
                "sf",
                &std::env::var("TASKS_DATABASE").unwrap(),
            ))
            .unwrap();
            let rows = symphony_board::Raw::of(&board)
                .table("chat_event")
                .fields(&["text"])
                .unwrap();
            let mut rows = rows.as_array().unwrap().clone();
            rows.sort_by(|a, b| a["text"].as_str().cmp(&b["text"].as_str()));
            assert_eq!(
                rows,
                [
                    json!({"text": "said before asks"}),
                    json!({"text": "too pale"})
                ]
            );
            assert!(f.store().asks(None).unwrap().is_empty());
        },
    );
}

#[test]
fn an_owner_save_in_a_projects_own_folder_commits_that_file_on_main() {
    isolated(
        "an_owner_save_in_a_projects_own_folder_commits_that_file_on_main",
        |f| {
            let root = f.repo();
            git(&root, &["mv", "docs", "manual"]);
            write(
                &root.join(symphony::project::FILE),
                &format!("[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"manual\"\n\n{VERDICTS}"),
            );
            git(&root, &["commit", "-qam", "the documents move to manual"]);
            let saves = ClosingSaves::start(&root);
            let physical = saves.0.post(
                "",
                "/api/decision",
                &json!({"path":"manual/a.md", "slug":"one", "state":"approved", "note":""}),
            );
            assert_eq!(
                physical.map_err(|failure| failure.error),
                Err("path must be a markdown file under docs/".to_string())
            );
            assert_eq!(
                saves.post(
                    "",
                    "/api/decision",
                    json!({"path":"docs/a.md", "slug":"one", "state":"approved", "note":"use this shape"}),
                ),
                json!({"ok":true})
            );
            saves.0.close();
            let text = std::fs::read_to_string(root.join("manual/a.md")).unwrap();
            assert!(text.contains("[x] **one**"), "{text}");
            assert_eq!(f.main("manual/a.md"), text);
            assert_eq!(
                git(
                    &root,
                    &["diff-tree", "--no-commit-id", "--name-only", "-r", "main"]
                ),
                "manual/a.md"
            );
            assert!(!root.join("docs").exists());
            assert!(git(&root, &["status", "--porcelain"]).is_empty());
            f.clean_workers();
        },
    );
}

/// A verdict waiting on an agent is an ask its journal record opens on the shared server's board:
/// no save, nor its close, runs a board command or writes the board. The board command this child
/// is given says so when anything runs it.
#[test]
fn verdict_saves_run_no_board_command_and_write_nothing_on_the_board() {
    isolated(
        "verdict_saves_run_no_board_command_and_write_nothing_on_the_board",
        |f| {
            // Plain-folder serving: no verdict commit, whose check names the app's session.
            let root = f.root();
            write(&root.join("docs/a.md"), DOC);
            write(&root.join("docs/b.md"), TWO);
            let wrapper = f.home.join("tasks-wrapper");
            let ran = f.home.join("board-command-ran");
            prepared_script(
                &wrapper,
                &format!(
                    "printf '%s\\n' \"$*\" >> {}\n\"$SYMPHONY_AFTER_COMMIT_TASKS\" \"$@\"",
                    quote(&ran)
                ),
            );
            // The board made, as the shared server keeps it, so that what a save wrote would show.
            let board = || {
                rpc::Client::new(
                    &std::env::var("TASKS_SERVER").unwrap(),
                    "sf",
                    &std::env::var("TASKS_DATABASE").unwrap(),
                )
            };
            symphony_board::Store::open(board()).unwrap();
            let log_path = f.home.join("saves.log");
            let mut child = OwnedChild::logged(
                sys::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "verdict_saves_case", "--nocapture"])
                    .env("SYMPHONY_TASKS_CLI", &wrapper)
                    .env("SYMPHONY_AFTER_COMMIT_VERDICT_SAVES", "1")
                    .stdin(sys::Stdio::Null),
                &log_path,
            );
            let status = child.0.wait().unwrap();
            child.finish_output();
            assert!(
                status.success(),
                "{}",
                std::fs::read_to_string(log_path).unwrap()
            );
            for (file, said) in [("docs/a.md", "redraw this"), ("docs/b.md", "fix this too")] {
                assert!(
                    std::fs::read_to_string(root.join(file))
                        .unwrap()
                        .contains(said),
                    "{file}"
                );
            }
            assert_eq!(
                std::fs::read_to_string(&ran).ok(),
                None,
                "a save ran the board command"
            );
            let board = symphony_board::Database::named(&board()).unwrap();
            let values = |table: &str, field: &str| {
                let rows = symphony_board::Raw::of(&board)
                    .table(table)
                    .fields(&[field]);
                let rows = rows.unwrap().as_array().unwrap().clone();
                serde_json::Value::Array(rows.iter().map(|row| row[field].clone()).collect())
            };
            let rows = [
                values("task", "slug"),
                values("session", "handle"),
                values("ask", "about"),
            ];
            assert_eq!(rows, [json!([]), json!([]), json!([])]);
            assert!(!root.join(".git").exists());
        },
    );
}

#[test]
fn verdict_saves_case() {
    if std::env::var_os("SYMPHONY_AFTER_COMMIT_VERDICT_SAVES").is_none() {
        return;
    }
    let f = Fixture {
        home: std::env::var(CHILD).unwrap().into(),
    };
    let saves = ClosingSaves::start(&f.root());
    saves.post(
        "",
        "/api/decision",
        json!({"path":"docs/a.md", "slug":"one", "state":"rejected", "note":"redraw this"}),
    );
    saves.post(
        "",
        "/api/decision",
        json!({"path":"docs/b.md", "slug":"two", "state":"rejected", "note":"fix this too"}),
    );
    saves.0.close();
}

#[test]
fn a_failed_callback_journal_retries_the_saved_comment_once() {
    isolated(
        "a_failed_callback_journal_retries_the_saved_comment_once",
        |f| {
            let root = f.repo();
            // A real storage failure after the document write, without replacing the save logic.
            let db = f.home.join("asks.db");
            std::fs::create_dir(&db).unwrap();
            let saves = ClosingSaves::start(&root);
            let payload = json!({"path":"docs/a.md", "after_line":1, "kind":"comment", "text":"Keep this outline"});
            let failure = saves
                .0
                .post("", "/api/comment", &payload)
                .expect_err("the journal cannot open a directory");
            assert!(failure.error.contains("sqlite3"), "{}", failure.error);
            let saved = std::fs::read_to_string(root.join("docs/a.md")).unwrap();
            assert_eq!(saved.matches("Keep this outline").count(), 1);
            std::fs::remove_dir(&db).unwrap();
            assert_eq!(saves.post("", "/api/comment", payload), json!({"ok":true}));
            assert_eq!(
                std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
                saved
            );
            saves.0.close();
            assert_eq!(f.main("docs/a.md"), saved);
            let mut flusher = symphony::callbacks::Flusher::new(&root);
            flusher.flush(&root).unwrap();
            flusher.flush(&root).unwrap();
            let board = symphony_board::Database::named(&rpc::Client::new(
                &std::env::var("TASKS_SERVER").unwrap(),
                "sf",
                &std::env::var("TASKS_DATABASE").unwrap(),
            ))
            .unwrap();
            let rows = symphony_board::Raw::of(&board)
                .table("chat_event")
                .fields(&["text"])
                .unwrap();
            assert_eq!(rows.as_array().unwrap().len(), 1, "{rows:?}");
            assert!(
                rows[0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Keep this outline")
            );
            f.clean_workers();
        },
    );
}

#[test]
fn worktree_saves_persist_locally_without_committing_to_main() {
    isolated(
        "worktree_saves_persist_locally_without_committing_to_main",
        |f| {
            let root = f.repo();
            let branch = f.home.join("branch");
            git(
                &root,
                &[
                    "worktree",
                    "add",
                    "-qb",
                    "work/proposal",
                    branch.to_str().unwrap(),
                ],
            );
            // The branch posts a card of its own: a card main holds as the branch shows it is
            // main's to answer (after_worktrees' `landed` cases).
            write(
                &branch.join("docs/a.md"),
                &DOC.replace(
                    "<!-- register:end -->",
                    "- [ ] **two** — the branch's proposal.\n<!-- register:end -->",
                ),
            );
            git(&branch, &["commit", "-qam", "post two"]);
            let tip = git(&root, &["rev-parse", "main"]);
            let branch_tip = git(&branch, &["rev-parse", "HEAD"]);
            let saves = ClosingSaves::start(&root);
            saves.post("work~proposal", "/api/decision", json!({"path":"docs/a.md", "slug":"two", "state":"approved", "note":"branch choice"}));
            saves.0.close();
            assert!(
                std::fs::read_to_string(branch.join("docs/a.md"))
                    .unwrap()
                    .contains("branch choice")
            );
            assert_eq!(f.main("docs/a.md"), DOC);
            assert_eq!(git(&root, &["rev-parse", "main"]), tip);
            assert_eq!(git(&branch, &["rev-parse", "HEAD"]), branch_tip);
            assert_eq!(git(&branch, &["status", "--porcelain"]), " M docs/a.md");
            git(
                &root,
                &["worktree", "remove", "--force", branch.to_str().unwrap()],
            );
            f.clean_workers();
        },
    );
}
