use super::{Files, PACKAGES, REPOSITORY, invalid};
use std::io;
use std::path::{Path, PathBuf};

fn lock_bytes(bytes: &[u8], mut expected: toml::Table) -> io::Result<Vec<u8>> {
    for package in expected
        .get_mut("package")
        .and_then(toml::Value::as_array_mut)
        .ok_or_else(|| invalid("missing lock packages"))?
    {
        if package
            .get("name")
            .and_then(toml::Value::as_str)
            .is_some_and(|name| PACKAGES.contains(&name))
        {
            package
                .as_table_mut()
                .ok_or_else(|| invalid("invalid package"))?
                .remove("source");
        }
    }
    let original = std::str::from_utf8(bytes).map_err(|e| invalid(e.to_string()))?;
    let mut output = String::new();
    let mut package = String::new();
    let mut in_package = false;
    let mut removed = 0;
    for line in original.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[[package]]";
            package.clear();
        } else if in_package && let Some((key, value)) = trimmed.split_once('=') {
            if key.trim() == "name" {
                let scalar: toml::Table = format!("value={value}")
                    .parse()
                    .map_err(|e: toml::de::Error| invalid(e.to_string()))?;
                package = scalar
                    .get("value")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| invalid("invalid lock package name"))?
                    .to_owned();
            } else if key.trim() == "source" && PACKAGES.contains(&package.as_str()) {
                removed += 1;
                continue;
            }
        }
        output.push_str(line);
    }
    let parsed: toml::Table = output
        .parse()
        .map_err(|e: toml::de::Error| invalid(e.to_string()))?;
    if removed != 2 || parsed != expected {
        return Err(invalid(
            "private component patch changed unrelated lock accounting",
        ));
    }
    Ok(output.into_bytes())
}

pub(super) fn configure(
    files: &mut Files,
    destination: &Path,
    lock: toml::Table,
    identity: &str,
) -> io::Result<()> {
    // Scoped selectors depend on the canonical snapshot path. Their immutable URLs
    // must change when that path changes, even if the original CSS is identical.
    for (path, (bytes, _)) in files.iter_mut() {
        if path.starts_with("source/apps/symphony-ui")
            && path.extension().is_some_and(|ext| ext == "css")
        {
            bytes.extend_from_slice(
                format!("\n:root{{--symphony-release-input:symphony-{identity};}}\n").as_bytes(),
            );
        }
    }
    let lock_path = Path::new("source/Cargo.lock");
    let patched = lock_bytes(
        &files
            .get(lock_path)
            .ok_or_else(|| invalid("missing lock input"))?
            .0,
        lock,
    )?;
    files.insert(lock_path.to_owned(), (patched, false));
    let config_path = PathBuf::from("source/.cargo/config.toml");
    let original = std::str::from_utf8(
        &files
            .get(&config_path)
            .ok_or_else(|| invalid("missing owning Cargo configuration"))?
            .0,
    )
    .map_err(|e| invalid(e.to_string()))?;
    let mut expected: toml::Table = original
        .parse()
        .map_err(|e: toml::de::Error| invalid(e.to_string()))?;
    if expected.contains_key("patch") {
        return Err(invalid("release inputs already contain Cargo patches"));
    }
    let mut output = format!("{original}\n[patch.'{REPOSITORY}']\n");
    let mut paths = toml::Table::new();
    for (package, folder) in [
        ("dioxus-attributes", "dioxus-attributes"),
        ("dioxus-primitives", "primitives"),
    ] {
        let path = destination
            .join("components")
            .join(folder)
            .to_string_lossy()
            .into_owned();
        let quoted = serde_json::to_string(&path).map_err(|e| invalid(e.to_string()))?;
        output.push_str(&format!("{package} = {{ path = {quoted} }}\n"));
        paths.insert(
            package.into(),
            toml::Value::Table(toml::Table::from_iter([(
                "path".into(),
                toml::Value::String(path),
            )])),
        );
    }
    expected.insert(
        "patch".into(),
        toml::Value::Table(toml::Table::from_iter([(
            REPOSITORY.into(),
            toml::Value::Table(paths),
        )])),
    );
    let parsed: toml::Table = output
        .parse()
        .map_err(|e: toml::de::Error| invalid(e.to_string()))?;
    if parsed != expected {
        return Err(invalid(
            "private component patch changed unrelated Cargo configuration",
        ));
    }
    files.insert(config_path, (output.into_bytes(), false));
    Ok(())
}
