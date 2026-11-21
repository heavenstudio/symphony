//! How a project gets the board's SurrealDB: the pins `tools/surreal.sh` holds, and the shell
//! [`surreal_fetch`] gives, run against a stand-in `curl` serving a tarball made here, and a
//! stand-in `tar` killed part way through its unpack.

use super::*;
use crate::children::Child as _;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// A folder of its own under the temporary one, for test `name`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("symphony-fetch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn executable(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_pins_are_the_ones_tools_surreal_sh_fetches_by() {
    let script =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/surreal.sh"))
            .unwrap();
    assert!(
        script.contains(&format!("\nVERSION={SURREAL_VERSION}\n")),
        "tools/surreal.sh pins another version"
    );
    for (_, _, build, sha) in SURREAL_BUILDS {
        assert!(
            script.contains(&format!("    {build}) echo {sha} ;;\n")),
            "{build}: {sha}"
        );
    }
    let pinned = script
        .lines()
        .filter(|line| line.contains(") echo "))
        .count();
    assert_eq!(
        pinned,
        SURREAL_BUILDS.len(),
        "a build pinned there and not here"
    );
}

#[test]
fn each_pinned_machine_gets_its_own_tarball_and_checker_and_any_other_none() {
    let surreal = Path::new("/r/.git/symphony/surreal/3.3.0/surreal");
    let mac = surreal_fetch(surreal, "macos", "aarch64").unwrap();
    assert!(
        mac.contains(&format!(
            "https://download.surrealdb.com/v{SURREAL_VERSION}/surreal-v{SURREAL_VERSION}.darwin-arm64.tgz"
        )),
        "{mac}"
    );
    assert!(mac.contains("| shasum -a 256 -c &&"), "{mac}");
    let linux = surreal_fetch(surreal, "linux", "x86_64").unwrap();
    assert!(linux.contains(".linux-amd64.tgz"), "{linux}");
    assert!(linux.contains("| sha256sum -c &&"), "{linux}");
    assert_eq!(surreal_fetch(surreal, "windows", "x86_64"), None);
    assert_eq!(surreal_fetch(surreal, "linux", "riscv64"), None);
}

#[test]
fn the_line_names_the_folder_once_and_stays_short_at_any_depth() {
    let deep = "/Users/someone/Documents/a long project name/with's a quote/.git/symphony/surreal";
    let surreal = Path::new(deep).join(SURREAL_VERSION).join("surreal");
    let line = surreal_fetch(&surreal, "macos", "aarch64").unwrap();
    let folder = format!(
        "/Users/someone/Documents/a long project name/with'\\''s a quote/.git/symphony/surreal/\
         {SURREAL_VERSION}"
    );
    assert!(line.starts_with(&format!("d='{folder}' && mkdir -p \"$d\" && ")), "{line}");
    assert_eq!(line.matches("someone").count(), 1, "{line}");
    assert!(line.contains(" && mv -f \"$d/.unpack/surreal\" \"$d/surreal\"; s=$?; "), "{line}");
    assert!(line.ends_with("; [ $s -eq 0 ]"), "{line}");
    // What the line adds to the folder's own length is fixed, whatever the path's depth.
    let own = line.len() - folder.len();
    assert!(own < 520, "{own}: {line}");
    assert!(!line.contains('\n'));
    // A binary's name the shell would read inside the line's double quotes gets no line.
    for name in ["sur$real", "sur real", "sur\"real", "sur`real"] {
        assert_eq!(fetch_line(Path::new("/r").join(name).as_path(), "b", "0", "linux"), None);
    }
    assert_eq!(fetch_line(Path::new("/"), "b", "0", "linux"), None);
}

/// The fetch run with `sha` as the pinned tarball's: a stand-in `curl` hands it `tarball`.
fn fetched(dir: &Path, surreal: &Path, tarball: &Path, sha: &str) -> std::process::Output {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let line = fetch_line(surreal, "test-build", sha, os).unwrap();
    // `curl -fsSLo FILE URL`: the URL noted, the tarball copied to FILE.
    executable(
        &dir.join("bin/curl"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$3\" >> '{}'\ncp '{}' \"$2\"\n",
            dir.join("curl.log").display(),
            tarball.display()
        ),
    );
    Command::of("/bin/sh")
        .args(["-c", &line])
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("bin").display()),
        )
        .output()
        .unwrap()
}

/// A stand-in `tar` on the fetch's search path, cut short as an interrupted unpack is: it leaves a
/// runnable part of `surreal` where it unpacks (`tar -xzf TGZ -C FOLDER surreal`) and is killed.
fn interrupted_tar(dir: &Path) -> PathBuf {
    let tar = dir.join("bin/tar");
    executable(
        &tar,
        "#!/bin/sh\nprintf '#!/bin/sh\\n# cut' > \"$4/surreal\"\nchmod +x \"$4/surreal\"\nkill -KILL $$\n",
    );
    tar
}

#[test]
fn the_fetch_installs_the_surreal_of_a_tarball_whose_sha256_is_the_pin_and_nothing_else() {
    let dir = scratch("install");
    executable(&dir.join("made/surreal"), "#!/bin/sh\necho surreal 3.3.0\n");
    let tarball = dir.join("made.tgz");
    let made = Command::of("tar")
        .args(["-czf"])
        .arg(&tarball)
        .arg("-C")
        .arg(dir.join("made"))
        .arg("surreal")
        .status()
        .unwrap();
    assert!(made.success());
    let sha: String = Sha256::digest(fs::read(&tarball).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    // A folder with a quote in its name, as a checkout's may have.
    let surreal = dir.join("it's common/symphony/surreal/3.3.0/surreal");
    let wrong = fetched(&dir, &surreal, &tarball, &"0".repeat(64));
    assert!(!wrong.status.success(), "{wrong:?}");
    assert!(
        !surreal.exists(),
        "a tarball that is not the pinned one installs nothing"
    );
    assert_eq!(
        fs::read_dir(surreal.parent().unwrap()).unwrap().count(),
        0,
        "and is removed"
    );
    // The pinned tarball, its unpack cut short: no part of a binary is where the board runs one.
    let tar = interrupted_tar(&dir);
    let cut = fetched(&dir, &surreal, &tarball, &sha);
    assert!(!cut.status.success(), "{cut:?}");
    assert!(!surreal.exists(), "an unpack cut short installs nothing");
    assert_eq!(
        fs::read_dir(surreal.parent().unwrap()).unwrap().count(),
        0,
        "its tarball and its part go with it"
    );
    fs::remove_file(&tar).unwrap();
    let done = fetched(&dir, &surreal, &tarball, &sha);
    assert!(done.status.success(), "{done:?}");
    assert!(crate::builds::runnable(&surreal));
    assert_eq!(
        fs::read_to_string(&surreal).unwrap(),
        "#!/bin/sh\necho surreal 3.3.0\n"
    );
    assert_eq!(
        fs::read_dir(surreal.parent().unwrap()).unwrap().count(),
        1,
        "the tarball and the unpack folder are removed"
    );
    // A fetch again, cut short, leaves the one there whole.
    let tar = interrupted_tar(&dir);
    assert!(!fetched(&dir, &surreal, &tarball, &sha).status.success());
    assert_eq!(
        fs::read_to_string(&surreal).unwrap(),
        "#!/bin/sh\necho surreal 3.3.0\n"
    );
    assert_eq!(fs::read_dir(surreal.parent().unwrap()).unwrap().count(), 1);
    fs::remove_file(&tar).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("curl.log")).unwrap(),
        format!(
            "https://download.surrealdb.com/v{SURREAL_VERSION}/surreal-v{SURREAL_VERSION}.test-build.tgz\n"
        )
        .repeat(4)
    );
    fs::remove_dir_all(&dir).unwrap();
}
