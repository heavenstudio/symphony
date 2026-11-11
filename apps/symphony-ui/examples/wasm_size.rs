//! Attribute an optimized, named WASM build without installing a profiler.
//!
//!     cargo run -p symphony-ui --example wasm_size -- NAMED.wasm
//!     cargo run -p symphony-ui --example wasm_size -- --functions [GROUP] NAMED.wasm
//!     cargo run -p symphony-ui --example wasm_size -- --sections MODULE.wasm
//!     cargo run -p symphony-ui --example wasm_size -- --strip-dwarf LINKED.wasm OUT.wasm
//!
//! The first prints each group's marginal gzip share, then the shares summed into kinds (Dioxus,
//! std, the app...). Names/debug sections are excluded. Marginal gzip bytes replace a group's
//! function bodies with traps in memory for compression only; these diagnostic copies are never
//! executed or written. Shares are not additive and exclude the group's strings in the data
//! section. `--functions` lists each function of a group (or every function), largest first, by
//! its raw code bytes and name. `--sections` prints each section's raw and gzip bytes, each
//! compressed on its own.
//! `--strip-dwarf` writes the module without its `.debug_*` sections, the step before the named
//! copies' wasm-bindgen and wasm-opt (probes/size.sh).

use std::collections::BTreeMap;
use std::io::Write;

use flate2::{Compression, write::GzEncoder};

#[derive(Clone)]
struct Section {
    id: u8,
    data: Vec<u8>,
}

struct Module {
    sections: Vec<Section>,
    bodies: Vec<Vec<u8>>,
    groups: Vec<String>,
    names: Vec<String>,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let (value, rest) = self.0.split_at_checked(count).ok_or("truncated WASM")?;
        self.0 = rest;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn integer(&mut self) -> Result<usize, String> {
        let mut value = 0_usize;
        for shift in (0..usize::BITS).step_by(7) {
            let byte = self.byte()?;
            let part = usize::from(byte & 127);
            if part > usize::MAX >> shift {
                return Err("WASM integer overflow".into());
            }
            value |= part << shift;
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err("WASM integer overflow".into())
    }

    fn string(&mut self) -> Result<String, String> {
        let length = self.integer()?;
        String::from_utf8(self.take(length)?.to_vec()).map_err(|e| e.to_string())
    }

    fn limits(&mut self) -> Result<(), String> {
        let flags = self.integer()?;
        self.integer()?;
        if flags & 1 != 0 {
            self.integer()?;
        }
        Ok(())
    }

    fn value_type(&mut self) -> Result<(), String> {
        if matches!(self.byte()?, 0x63 | 0x64) {
            self.integer()?;
        }
        Ok(())
    }
}

fn integer(mut value: usize, out: &mut Vec<u8>) {
    loop {
        let rest = value >> 7;
        out.push(u8::try_from(value & 127).expect("seven bits") | if rest == 0 { 0 } else { 128 });
        value = rest;
        if rest == 0 {
            break;
        }
    }
}

const MAGIC: &[u8; 8] = b"\0asm\x01\0\0\0";

/// Each section of a version-one module in order: its id and its payload.
fn sections(bytes: &[u8]) -> Result<Vec<(u8, &[u8])>, String> {
    let mut reader = Reader(bytes);
    if reader.take(8)? != MAGIC {
        return Err("not a version-one WASM module".into());
    }
    let mut found = Vec::new();
    while !reader.0.is_empty() {
        let id = reader.byte()?;
        let length = reader.integer()?;
        found.push((id, reader.take(length)?));
    }
    Ok(found)
}

/// A section as a reader names it: a custom section by its own name.
fn section_name(id: u8, data: &[u8]) -> Result<String, String> {
    const NAMES: [&str; 14] = [
        "custom",
        "type",
        "import",
        "function",
        "table",
        "memory",
        "global",
        "export",
        "start",
        "element",
        "code",
        "data",
        "data count",
        "tag",
    ];
    if id == 0 {
        return Ok(format!("custom {}", Reader(data).string()?));
    }
    Ok(NAMES
        .get(usize::from(id))
        .map_or_else(|| format!("unknown {id}"), |name| (*name).to_owned()))
}

fn section_report(bytes: &[u8], out: &mut impl Write) -> Result<(), String> {
    writeln!(out, "      raw     gzip section (each gzipped on its own)")
        .map_err(|e| e.to_string())?;
    for (id, data) in sections(bytes)? {
        let name = section_name(id, data)?;
        writeln!(out, "{:>9} {:>8} {name}", data.len(), gzip(data)?).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The module without its DWARF (`.debug_*` custom sections), every other section as it was.
fn without_dwarf(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = MAGIC.to_vec();
    for (id, data) in sections(bytes)? {
        if id == 0 && Reader(data).string()?.starts_with(".debug_") {
            continue;
        }
        out.push(id);
        integer(data.len(), &mut out);
        out.extend_from_slice(data);
    }
    Ok(out)
}

/// What a group is part of, for the summary under the groups.
fn kind(group: &str) -> &'static str {
    const FRAMEWORK: [&str; 14] = [
        "dioxus",
        "generational_box",
        "sledgehammer",
        "subsecond",
        "slab",
        "slotmap",
        "rustc_hash",
        "smallvec",
        "parking_lot",
        "lock_api",
        "futures",
        "tracing",
        "once_cell",
        "hashbrown",
    ];
    let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| group.starts_with(prefix));
    if group.starts_with("symphony_ui::") {
        "the app's own modules (symphony_ui)"
    } else if starts(&FRAMEWORK) {
        "Dioxus and its runtime crates"
    } else if starts(&["symphony_api", "serde"]) {
        "wire types (serde derives, serde-wasm-bindgen)"
    } else if starts(&["anyhow", "rustc_demangle", "std::backtrace"]) {
        "anyhow and the backtraces it prints"
    } else if starts(&["core::", "alloc", "std::", "dlmalloc", "___rust"]) {
        "std, core, alloc and the allocator"
    } else if starts(&["js_sys", "web_sys", "wasm_bindgen", "non-Rust"]) {
        "bindings (wasm-bindgen, js-sys, web-sys)"
    } else if group == "unnamed" {
        "unnamed"
    } else {
        "other"
    }
}

fn imported_functions(data: &[u8]) -> Result<usize, String> {
    let mut reader = Reader(data);
    let mut functions = 0;
    for _ in 0..reader.integer()? {
        reader.string()?;
        reader.string()?;
        match reader.byte()? {
            0 => {
                reader.integer()?;
                functions += 1;
            }
            1 => {
                reader.value_type()?;
                reader.limits()?;
            }
            2 => reader.limits()?,
            3 => {
                reader.value_type()?;
                reader.byte()?;
            }
            4 => {
                reader.byte()?;
                reader.integer()?;
            }
            kind => return Err(format!("unsupported WASM import kind {kind}")),
        }
    }
    Ok(functions)
}

fn function_names(data: &[u8], names: &mut BTreeMap<usize, String>) -> Result<(), String> {
    let mut reader = Reader(data);
    if reader.string()? != "name" {
        return Ok(());
    }
    while !reader.0.is_empty() {
        let kind = reader.byte()?;
        let length = reader.integer()?;
        let mut subsection = Reader(reader.take(length)?);
        if kind == 1 {
            for _ in 0..subsection.integer()? {
                names.insert(subsection.integer()?, subsection.string()?);
            }
        }
    }
    Ok(())
}

fn identifier(text: &str) -> Option<(&str, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let length: usize = text.get(..digits)?.parse().ok()?;
    let (name, rest) = text.get(digits..)?.split_at_checked(length)?;
    Some((name, rest))
}

fn group(name: &str) -> String {
    let mut crates = Vec::new();
    for (at, _) in name.match_indices('C') {
        let Some(mut tail) = name.get(at + 1..) else {
            continue;
        };
        if tail.starts_with('s') {
            let Some((_, rest)) = tail.split_once('_') else {
                continue;
            };
            tail = rest;
        }
        if let Some(found) = identifier(tail) {
            crates.push(found);
        }
    }
    let Some(&(mut origin, mut tail)) = crates.first() else {
        return "non-Rust".into();
    };
    // Dioxus's component trampoline belongs with the component, not its tiny HotFunction shim.
    if origin == "subsecond"
        && let Some(&(found, rest)) = crates.iter().find(|(c, _)| *c == "symphony_ui")
    {
        origin = found;
        tail = rest;
    }
    if matches!(origin, "symphony_ui" | "std" | "core")
        && let Some((module, _)) = identifier(tail)
    {
        return format!("{origin}::{module}");
    }
    origin.into()
}

impl Module {
    fn read(bytes: &[u8]) -> Result<Self, String> {
        let mut sections = Vec::new();
        let mut names = BTreeMap::new();
        let mut bodies = Vec::new();
        let mut imports = 0;
        for (id, data) in self::sections(bytes)? {
            match id {
                0 => function_names(data, &mut names)?,
                2 => imports = imported_functions(data)?,
                10 => {
                    let mut code = Reader(data);
                    for _ in 0..code.integer()? {
                        let length = code.integer()?;
                        bodies.push(code.take(length)?.to_vec());
                    }
                    if !code.0.is_empty() {
                        return Err("trailing bytes in WASM code section".into());
                    }
                }
                _ => {}
            }
            if id != 0 {
                sections.push(Section {
                    id,
                    data: data.to_vec(),
                });
            }
        }
        if bodies.is_empty() || names.is_empty() {
            return Err("attribution needs code and a function-name section".into());
        }
        let names: Vec<String> = (imports..imports + bodies.len())
            .map(|index| names.get(&index).cloned().unwrap_or_default())
            .collect();
        let groups = names
            .iter()
            .map(|name| {
                if name.is_empty() {
                    "unnamed".into()
                } else {
                    group(name)
                }
            })
            .collect();
        Ok(Self {
            sections,
            bodies,
            groups,
            names,
        })
    }

    fn bytes_without(&self, removed: Option<&str>) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for section in &self.sections {
            out.push(section.id);
            if section.id == 10 {
                let mut code = Vec::new();
                integer(self.bodies.len(), &mut code);
                for (body, group) in self.bodies.iter().zip(&self.groups) {
                    let body = if removed == Some(group) {
                        &[0, 0, 0x0b]
                    } else {
                        body.as_slice()
                    };
                    integer(body.len(), &mut code);
                    code.extend_from_slice(body);
                }
                integer(code.len(), &mut out);
                out.extend_from_slice(&code);
            } else {
                integer(section.data.len(), &mut out);
                out.extend_from_slice(&section.data);
            }
        }
        out
    }
}

fn gzip(bytes: &[u8]) -> Result<usize, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).map_err(|e| e.to_string())?;
    Ok(encoder.finish().map_err(|e| e.to_string())?.len())
}

fn report(bytes: &[u8], out: &mut impl Write) -> Result<(), String> {
    let module = Module::read(bytes)?;
    let base = gzip(&module.bytes_without(None))?;
    let mut groups: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (body, group) in module.bodies.iter().zip(&module.groups) {
        let (count, size) = groups.entry(group).or_default();
        *count += 1;
        *size += body.len();
    }
    let mut sizes = groups
        .into_iter()
        .map(|(group, (count, size))| {
            let without = gzip(&module.bytes_without(Some(group)))?;
            Ok((
                i64::try_from(base).map_err(|e| e.to_string())?
                    - i64::try_from(without).map_err(|e| e.to_string())?,
                size,
                count,
                group,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    sizes.sort_by(|a, b| b.cmp(a));
    writeln!(out, "Named optimized module, custom sections excluded: {base} gzip bytes\nMarginal gzip shares are non-additive; data strings stay in the module.\n gzip saved   raw code functions group").map_err(|e| e.to_string())?;
    let mut kinds: BTreeMap<&str, i64> = BTreeMap::new();
    for (saved, size, count, group) in &sizes {
        writeln!(out, "{saved:>11} {size:>10} {count:>9} {group}").map_err(|e| e.to_string())?;
        *kinds.entry(kind(group)).or_default() += saved;
    }
    let mut kinds: Vec<_> = kinds
        .into_iter()
        .map(|(kind, saved)| (saved, kind))
        .collect();
    kinds.sort_by(|a, b| b.cmp(a));
    let total: i64 = kinds.iter().map(|(saved, _)| saved).sum();
    writeln!(
        out,
        "\nShares summed into kinds (the sum of shares is {total}):"
    )
    .map_err(|e| e.to_string())?;
    for (saved, kind) in kinds {
        writeln!(out, "{saved:>11} {kind}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Every function of `group` (all of them when empty), largest first: its raw code bytes and name.
fn function_report(bytes: &[u8], group: &str, out: &mut impl Write) -> Result<(), String> {
    let module = Module::read(bytes)?;
    let mut functions: Vec<_> = module
        .bodies
        .iter()
        .zip(&module.groups)
        .zip(&module.names)
        .filter(|((_, found), _)| group.is_empty() || found.as_str() == group)
        .map(|((body, found), name)| (body.len(), found, name))
        .collect();
    functions.sort_by(|a, b| b.cmp(a));
    writeln!(out, "  raw code group name").map_err(|e| e.to_string())?;
    for (size, found, name) in functions {
        writeln!(out, "{size:>10} {found} {name}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

const USAGE: &str = "usage: wasm_size NAMED.wasm | wasm_size --functions [GROUP] NAMED.wasm | wasm_size --sections MODULE.wasm | wasm_size --strip-dwarf LINKED.wasm OUT.wasm";

fn main() -> Result<(), String> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let read = |path: &std::ffi::OsString| {
        std::fs::read(path).map_err(|e| format!("{}: {e}", std::path::Path::new(path).display()))
    };
    match args.as_slice() {
        [flag, path] if flag == "--sections" => {
            section_report(&read(path)?, &mut std::io::stdout().lock())
        }
        [flag, path] if flag == "--functions" => {
            function_report(&read(path)?, "", &mut std::io::stdout().lock())
        }
        [flag, group, path] if flag == "--functions" => function_report(
            &read(path)?,
            &group.to_string_lossy(),
            &mut std::io::stdout().lock(),
        ),
        [flag, from, to] if flag == "--strip-dwarf" => {
            std::fs::write(to, without_dwarf(&read(from)?)?)
                .map_err(|e| format!("{}: {e}", std::path::Path::new(to).display()))
        }
        [path] if !path.to_string_lossy().starts_with("--") => {
            report(&read(path)?, &mut std::io::stdout().lock())
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: u8, data: &[u8], out: &mut Vec<u8>) {
        out.push(id);
        integer(data.len(), out);
        out.extend_from_slice(data);
    }

    fn fixture() -> Vec<u8> {
        let mut out = b"\0asm\x01\0\0\0".to_vec();
        section(2, &[1, 1, b'm', 1, b'f', 0, 0], &mut out);
        section(10, &[1, 4, 0, 0x41, 42, 0x0b], &mut out);
        let name = "_RNvNtCs123_11symphony_ui5frame5Frame";
        let mut names = vec![1, 1]; // one name, function index one (after the import)
        integer(name.len(), &mut names);
        names.extend_from_slice(name.as_bytes());
        let mut custom = vec![4, b'n', b'a', b'm', b'e', 1];
        integer(names.len(), &mut custom);
        custom.extend_from_slice(&names);
        section(0, &custom, &mut out);
        out
    }

    #[test]
    fn names_account_for_imports_and_diagnostic_replacement_preserves_other_sections() {
        let module = Module::read(&fixture()).unwrap();
        assert_eq!(module.groups, ["symphony_ui::frame"]);
        let stripped = module.bytes_without(None);
        assert!(!stripped.windows(4).any(|w| w == b"name"));
        let replaced = module.bytes_without(Some("symphony_ui::frame"));
        assert!(replaced.ends_with(&[10, 5, 1, 3, 0, 0, 0x0b]));
        assert_eq!(replaced.get(8..17), stripped.get(8..17));
        let mut output = Vec::new();
        report(&fixture(), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        let (groups, kinds) = output.split_once("\n\n").unwrap();
        assert!(
            groups
                .lines()
                .last()
                .unwrap()
                .ends_with(" symphony_ui::frame")
        );
        let saved = groups
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_eq!(
            kinds.lines().collect::<Vec<_>>(),
            [
                format!("Shares summed into kinds (the sum of shares is {saved}):"),
                format!("{saved:>11} the app's own modules (symphony_ui)"),
            ]
        );
    }

    #[test]
    fn the_function_listing_names_each_body_of_the_group_asked_for() {
        let name = "_RNvNtCs123_11symphony_ui5frame5Frame";
        let listing = |group: &str| {
            let mut output = Vec::new();
            function_report(&fixture(), group, &mut output).unwrap();
            String::from_utf8(output).unwrap()
        };
        let every = listing("");
        assert_eq!(
            every.lines().collect::<Vec<_>>(),
            [
                "  raw code group name".to_string(),
                format!("{:>10} symphony_ui::frame {name}", 4),
            ]
        );
        assert_eq!(listing("symphony_ui::frame"), every);
        assert_eq!(listing("dioxus_core").lines().count(), 1);
    }

    #[test]
    fn kinds_put_each_group_with_what_it_is_part_of() {
        let app = "the app's own modules (symphony_ui)";
        let framework = "Dioxus and its runtime crates";
        let wire = "wire types (serde derives, serde-wasm-bindgen)";
        let errors = "anyhow and the backtraces it prints";
        let runtime = "std, core, alloc and the allocator";
        let bindings = "bindings (wasm-bindgen, js-sys, web-sys)";
        for (group, expected) in [
            ("symphony_ui::live", app),
            ("dioxus_core", framework),
            ("generational_box", framework),
            ("futures_util", framework),
            ("hashbrown", framework),
            ("symphony_api", wire),
            ("serde_wasm_bindgen", wire),
            ("anyhow", errors),
            ("rustc_demangle", errors),
            ("std::backtrace", errors),
            ("std::io", runtime),
            ("core::fmt", runtime),
            ("alloc", runtime),
            ("dlmalloc", runtime),
            ("wasm_bindgen_futures", bindings),
            ("web_sys", bindings),
            ("non-Rust", bindings),
            ("unnamed", "unnamed"),
            ("itoa", "other"),
        ] {
            assert_eq!(kind(group), expected, "{group}");
        }
    }

    #[test]
    fn sections_are_reported_by_name_and_only_the_dwarf_is_stripped() {
        let mut module = fixture();
        let mut dwarf = Vec::new();
        integer(".debug_info".len(), &mut dwarf);
        dwarf.extend_from_slice(b".debug_info");
        dwarf.extend_from_slice(&[1, 2, 3]);
        section(0, &dwarf, &mut module);
        let mut output = Vec::new();
        section_report(&module, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        let rows: Vec<(usize, String)> = output
            .lines()
            .skip(1)
            .map(|line| {
                let mut words = line.split_whitespace();
                let raw = words.next().unwrap().parse().unwrap();
                words.next().unwrap().parse::<usize>().unwrap();
                (raw, words.collect::<Vec<_>>().join(" "))
            })
            .collect();
        let name_length = sections(&fixture()).unwrap()[2].1.len();
        assert_eq!(
            rows,
            [
                (7, "import".to_owned()),
                (6, "code".to_owned()),
                (name_length, "custom name".to_owned()),
                (15, "custom .debug_info".to_owned()),
            ]
        );
        assert_eq!(without_dwarf(&module).unwrap(), fixture());
        assert_eq!(without_dwarf(&fixture()).unwrap(), fixture());
        assert!(without_dwarf(b"notwasm!").is_err());
        assert!(section_report(&b"\0asm\x01\0\0\0\x01\x05"[..], &mut Vec::new()).is_err());
        assert_eq!(section_name(13, &[]).unwrap(), "tag");
        assert_eq!(section_name(99, &[]).unwrap(), "unknown 99");
    }

    #[test]
    fn grouping_uses_the_origin_and_assigns_component_trampolines_to_the_page() {
        for (name, expected) in [
            (
                "_RINvMNtNtCs9w_11dioxus_core4diffNtNtCs12_10dioxus_web",
                "dioxus_core",
            ),
            (
                "_RNvXsf_Cs1_9subsecondNCINvMsu_NtCs2_11symphony_ui5tasks",
                "symphony_ui::tasks",
            ),
            ("_RNvNtCs1_3std9backtrace7capture", "std::backtrace"),
            ("__wbindgen_malloc", "non-Rust"),
        ] {
            assert_eq!(group(name), expected, "{name}");
        }
    }

    #[test]
    fn truncated_invalid_and_unnamed_input_is_refused() {
        for bytes in [
            &b"bad"[..],
            &b"notwasm!"[..],
            &b"\0asm\x01\0\0\0"[..],
            &b"\0asm\x01\0\0\0\x01\x80"[..],
        ] {
            assert!(Module::read(bytes).is_err(), "{bytes:?}");
        }
        assert!(Reader(&[255; 20]).integer().is_err());
        assert!(
            imported_functions(&[1, 0, 0, 99])
                .unwrap_err()
                .contains("99")
        );
        for value in [0, 127, 128, 16384, usize::MAX] {
            let mut bytes = Vec::new();
            integer(value, &mut bytes);
            assert_eq!(Reader(&bytes).integer().unwrap(), value);
        }
    }

    #[test]
    fn non_function_imports_do_not_shift_function_names() {
        let imports = [
            5, 0, 0, 1, 0x70, 0, 1, 0, 0, 2, 1, 1, 2, 0, 0, 3, 0x7f, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(imported_functions(&imports).unwrap(), 1);
    }
}
