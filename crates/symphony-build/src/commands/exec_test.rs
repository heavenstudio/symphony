//! Cargo's target-runner adapter for the release step's shared library fixtures. Cargo supplies
//! the artifact, cwd, environment (including dynamic-library paths) and libtest arguments. The
//! map made from the one workspace build selects the shared libraries; nextest runs the others.
//! No shell sits between Cargo and the library image, so macOS keeps its dynamic-library paths.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Write;

use crate::replace::replace;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    binary: String,
    shared: bool,
}

pub(super) fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        let _ = writeln!(
            out,
            "usage: symphony-build exec-test-binary MAP BINARY [ARGS]..."
        );
        return 0;
    }
    let [map, binary, rest @ ..] = args else {
        let _ = writeln!(
            err,
            "usage: symphony-build exec-test-binary MAP BINARY [ARGS]..."
        );
        return 2;
    };
    let selected = std::fs::read(map)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<Vec<Artifact>>(&bytes).map_err(|error| error.to_string())
        });
    match selected {
        Ok(selected) => {
            let mut known = BTreeSet::new();
            for artifact in &selected {
                if !known.insert(&artifact.binary) {
                    let _ = writeln!(
                        err,
                        "exec-test-binary: ambiguous library artifact {} in {map}",
                        artifact.binary
                    );
                    return 2;
                }
            }
            match selected.iter().find(|artifact| artifact.binary == *binary) {
                Some(Artifact { shared: false, .. }) => 0,
                Some(Artifact { shared: true, .. }) => {
                    let error = replace(OsStr::new(binary), rest, &[]);
                    let _ = writeln!(err, "exec-test-binary: cannot execute {binary}: {error}");
                    2
                }
                None => {
                    let _ = writeln!(
                        err,
                        "exec-test-binary: {binary} is not a library artifact in {map}"
                    );
                    2
                }
            }
        }
        Err(error) => {
            let _ = writeln!(err, "exec-test-binary: cannot read {map}: {error}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    fn run(args: &[String], err: &mut dyn Write) -> i32 {
        main(args, &mut Vec::new(), err)
    }

    #[test]
    fn help_needs_no_selection_file_and_writes_only_stdout() {
        for flag in ["-h", "--help"] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(main(&[flag.into()], &mut out, &mut err), 0);
            assert_eq!(
                out,
                b"usage: symphony-build exec-test-binary MAP BINARY [ARGS]...\n"
            );
            assert!(err.is_empty());
        }
    }

    #[test]
    fn only_an_explicit_known_stateless_library_is_left_to_nextest() {
        let tree = Tree::new();
        let map = tree.write("map.json", r#"[{"binary":"/known stateless","shared":false},{"binary":"/shared missing","shared":true}]"#);
        let map = map.to_str().unwrap().to_string();
        let mut err = Vec::new();
        assert_eq!(run(&[map.clone(), "/known stateless".into()], &mut err), 0);
        assert!(err.is_empty());
        assert_eq!(run(&[map.clone(), "/unknown".into()], &mut err), 2);
        assert!(String::from_utf8_lossy(&err).contains("not a library artifact"));
        err.clear();
        assert_eq!(run(&[map, "/shared missing".into()], &mut err), 2);
        assert!(String::from_utf8_lossy(&err).contains("cannot execute /shared missing"));
    }

    #[test]
    fn missing_malformed_and_unreadable_maps_refuse() {
        let tree = Tree::new();
        for args in [vec![], vec!["one".into()]] {
            let mut err = Vec::new();
            assert_eq!(run(&args, &mut err), 2);
            assert!(String::from_utf8_lossy(&err).starts_with("usage:"));
        }
        for content in ["{", r#"[{"binary":"/known","shared":"yes"}]"#] {
            let map = tree.write("bad.json", content);
            let mut err = Vec::new();
            assert_eq!(
                run(&[map.display().to_string(), "/known".into()], &mut err),
                2
            );
            assert!(String::from_utf8_lossy(&err).contains("cannot read"));
        }
        let mut err = Vec::new();
        assert_eq!(
            run(
                &[tree.path("absent").display().to_string(), "/known".into()],
                &mut err
            ),
            2
        );
    }

    #[test]
    fn duplicate_artifacts_are_ambiguous_even_if_their_selection_agrees() {
        let tree = Tree::new();
        for shared in [true, false] {
            let map = tree.write(
                "duplicates.json",
                &serde_json::json!([
                    {"binary":"/known", "shared":false}, {"binary":"/known", "shared":shared}
                ])
                .to_string(),
            );
            let mut err = Vec::new();
            assert_eq!(
                run(&[map.display().to_string(), "/known".into()], &mut err),
                2
            );
            assert!(String::from_utf8_lossy(&err).contains("ambiguous library artifact"));
        }
    }
}
