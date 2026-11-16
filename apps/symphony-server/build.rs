#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn files(root: &Path, directory: &Path, found: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    println!("cargo:rerun-if-changed={}", directory.display());
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            files(root, &path, found)?;
        } else if kind.is_file() {
            let name = path.strip_prefix(root).map_err(io::Error::other)?;
            let name = name
                .to_str()
                .ok_or_else(|| io::Error::other("non-UTF-8 client filename"))?;
            found.push((name.to_owned(), path));
        } else {
            return Err(io::Error::other(format!(
                "{}: client assets must be regular files",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Clippy's `max-include-file-size` (its default; clippy.toml leaves it): `large_include_file`
/// fires on an `include_bytes!` of a file longer than this, so the manifest expects the lint only
/// while the distribution holds such a file: none today, the largest being the chunk every kind
/// shares.
const INCLUDE_LIMIT: u64 = 1_000_000;

fn generate(root: &Path) -> io::Result<String> {
    let present = match fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => true,
        Ok(_) => {
            return Err(io::Error::other(format!(
                "{}: client assets must be a directory",
                root.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(io::Error::other(format!("{}: {error}", root.display()))),
    };
    let mut assets = Vec::new();
    if present {
        files(root, root, &mut assets)?;
    }
    assets.sort_by(|left, right| left.0.cmp(&right.0));
    if !assets.is_empty() && !assets.iter().any(|(name, _)| name == "index.html") {
        return Err(io::Error::other(format!(
            "{}: shipped client has no index.html",
            root.display()
        )));
    }
    let mut generated = String::new();
    for (_, file) in &assets {
        if fs::metadata(file)?.len() > INCLUDE_LIMIT {
            generated.push_str("#[expect(clippy::large_include_file, reason = \"The server embeds the actual client WASM so installed consumers need no client compiler or source checkout\")]\n");
            break;
        }
    }
    generated.push_str("pub(super) static FILES: &[(&str, &[u8])] = &[\n");
    for (name, file) in assets {
        println!("cargo:rerun-if-changed={}", file.display());
        generated.push_str(&format!("({name:?}, include_bytes!({file:?})),\n"));
    }
    generated.push_str("];\n");
    Ok(generated)
}

#[cfg_attr(
    test,
    expect(
        dead_code,
        reason = "Native acceptance tests include the real generator; Cargo alone calls its entrypoint"
    )
)]
fn main() -> io::Result<()> {
    // No other code loads a binary of this package, so on macOS each links with no export trie, a
    // name for each global symbol that would serve only a debugger reading a stripped build, which
    // a build to debug replaces with one that keeps every name
    // (`CARGO_PROFILE_RELEASE_STRIP=debuginfo`; tests/links.rs; docs/CHECKS.md, "Size budgets").
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-no_exported_symbols");
    }
    let root = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| io::Error::other("missing manifest directory"))?,
    )
    .join("client/public");
    let generated = generate(&root)?;
    let output = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing build output directory"))?,
    );
    fs::write(output.join("client.rs"), generated)
}

#[cfg(test)]
mod tests {
    use super::{INCLUDE_LIMIT, generate};
    use crate::tree::Tree;

    const EXPECTATION: &str = "#[expect(clippy::large_include_file";

    #[test]
    fn the_manifest_expects_the_large_include_lint_only_over_clippys_limit() {
        let tree = Tree::new();
        let root = tree.path("release");
        assert!(!generate(&root).unwrap().contains(EXPECTATION));
        tree.write("release/index.html", "protocol input only");
        let at_limit = "x".repeat(usize::try_from(INCLUDE_LIMIT).unwrap());
        tree.write("release/assets/program.wasm", &at_limit);
        let manifest = generate(&root).unwrap();
        assert!(!manifest.contains(EXPECTATION), "{manifest}");
        assert!(
            manifest.starts_with("pub(super) static FILES"),
            "{manifest}"
        );
        tree.write("release/assets/program.wasm", &format!("{at_limit}x"));
        let manifest = generate(&root).unwrap();
        assert!(manifest.starts_with(EXPECTATION), "{manifest}");
        assert_eq!(manifest.matches(EXPECTATION).count(), 1, "{manifest}");
        tree.write("release/assets/second.wasm", &format!("{at_limit}x"));
        let manifest = generate(&root).unwrap();
        assert_eq!(manifest.matches(EXPECTATION).count(), 1, "{manifest}");
    }

    #[test]
    fn absent_assets_allow_bootstrap_but_a_partial_release_refuses() {
        let tree = Tree::new();
        let root = tree.path("release");
        assert!(generate(&root).unwrap().ends_with("= &[\n];\n"));
        tree.write("release/assets/module.js", "protocol input only");
        let error = generate(&root).unwrap_err().to_string();
        assert!(
            error.contains("release: shipped client has no index.html"),
            "{error}"
        );
    }

    #[test]
    fn manifest_orders_owned_regular_files_and_rejects_links() {
        let tree = Tree::new();
        let root = tree.path("release");
        tree.write("release/index.html", "protocol input only");
        tree.write("release/assets/z.js", "last");
        tree.write("release/assets/a.js", "first");
        let manifest = generate(&root).unwrap();
        assert!(manifest.find("assets/a.js").unwrap() < manifest.find("assets/z.js").unwrap());
        assert!(manifest.find("assets/z.js").unwrap() < manifest.find("index.html").unwrap());
        let outside = tree.write("outside.js", "not owned");
        std::os::unix::fs::symlink(outside, root.join("assets/link.js")).unwrap();
        let error = generate(&root).unwrap_err().to_string();
        assert!(
            error.contains("assets/link.js: client assets must be regular files"),
            "{error}"
        );
        std::os::unix::fs::symlink(&root, tree.path("release-link")).unwrap();
        let error = generate(&tree.path("release-link"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("release-link: client assets must be a directory"),
            "{error}"
        );
    }
}
