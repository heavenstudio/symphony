//! Filesystem checkout layouts used by the heartbeat.
use crate::test_tree::Tree as Tmp;
use std::path::Path;
use symphony_board::git::checkout_common_dir;

#[test]
fn a_checkouts_common_dir_is_read_from_its_own_marker_alone() {
    let mut tmp = Tmp::new();
    tmp.root = tmp.root.canonicalize().unwrap();
    let write = |path: &Path, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let main = tmp.path("main");
    // A linked worktree's `gitdir:` names its folder in main's `.git`, whose `commondir` leads back.
    write(&main.join(".git/worktrees/side/commondir"), "../..\n");
    let side = tmp.path("side");
    let admin = main.join(".git/worktrees/side");
    write(
        &side.join(".git"),
        &format!("gitdir: {}\n", admin.display()),
    );
    // A submodule's relative `gitdir:` names a folder with no `commondir`.
    std::fs::create_dir_all(main.join(".git/modules/sub")).unwrap();
    write(&main.join("sub/.git"), "gitdir: ../.git/modules/sub\n");
    // Folders inside main whose `.git` names nothing, and one with none.
    std::fs::create_dir_all(main.join("dangling")).unwrap();
    std::os::unix::fs::symlink("missing", main.join("dangling/.git")).unwrap();
    write(&main.join("no-gitdir/.git"), "not a pointer\n");
    write(&main.join("gone/.git"), "gitdir: ../missing\n");
    write(&main.join("file-admin"), "not a directory\n");
    write(&main.join("file-gitdir/.git"), "gitdir: ../file-admin\n");
    write(
        &main.join("invalid-common/.git"),
        "gitdir: ../invalid-admin\n",
    );
    std::fs::create_dir_all(main.join("invalid-admin/commondir")).unwrap();
    std::fs::create_dir_all(main.join("unmarked")).unwrap();

    let common = |top: &Path| checkout_common_dir(top);
    assert_eq!(common(&main), Some(main.join(".git")));
    assert_eq!(common(&side), Some(main.join(".git")));
    assert_eq!(
        common(&main.join("sub")),
        Some(main.join(".git/modules/sub"))
    );
    for top in [
        "dangling",
        "no-gitdir",
        "gone",
        "unmarked",
        "file-gitdir",
        "invalid-common",
    ] {
        assert_eq!(common(&main.join(top)), None, "{top}");
    }
}
