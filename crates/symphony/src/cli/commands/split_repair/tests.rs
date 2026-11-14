//! The repair on small programs built byte by byte to show each fault as dx's splitter makes it:
//! the main module it writes with a chunk's merged constant zeroed and its zero runs packed away as
//! wasm-opt packs them, and the chunks two runs write with the same segments in two orders.

use super::*;
use crate::test_tree::Tree;
use wasm::{write_i32, write_u32};

/// A section: its id and its body.
type Built = (u8, Vec<u8>);

fn module(sections: &[Built]) -> Vec<u8> {
    let mut out = b"\0asm\x01\0\0\0".to_vec();
    for (id, body) in sections {
        out.push(*id);
        write_u32(&mut out, body.len() as u32);
        out.extend_from_slice(body);
    }
    out
}

fn name(out: &mut Vec<u8>, name: &str) {
    write_u32(out, name.len() as u32);
    out.extend_from_slice(name.as_bytes());
}

fn custom(called: &str, contents: &[u8]) -> Built {
    let mut body = Vec::new();
    name(&mut body, called);
    body.extend_from_slice(contents);
    (0, body)
}

fn vector(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    write_u32(&mut out, items.len() as u32);
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// `i32.const value`, `end`.
fn at(value: i32) -> Vec<u8> {
    let mut out = vec![0x41];
    write_i32(&mut out, value);
    out.push(0x0b);
    out
}

/// A data segment active in memory 0 at `address`.
fn active(address: i32, bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0];
    out.extend(at(address));
    write_u32(&mut out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    out
}

fn passive(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    write_u32(&mut out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    out
}

/// An element segment active in table 0 at `slot`, of these functions.
fn entries(slot: i32, functions: &[u32]) -> Vec<u8> {
    let mut out = vec![0];
    out.extend(at(slot));
    write_u32(&mut out, functions.len() as u32);
    for function in functions {
        write_u32(&mut out, *function);
    }
    out
}

/// One memory of one page.
fn memory() -> Built {
    (5, vec![1, 0x00, 1])
}

/// What a chunk imports from the main module: its memory and its table.
fn chunk_imports() -> Built {
    let mut body = Vec::new();
    write_u32(&mut body, 2);
    name(&mut body, "__wasm_split");
    name(&mut body, "memory");
    body.extend([0x02, 0x00, 1]);
    name(&mut body, "__wasm_split");
    name(&mut body, "__indirect_function_table");
    body.extend([0x01, 0x70, 0x01, 4, 64]);
    (2, body)
}

/// A main module's own import from the page's bindings, which is no chunk's.
fn main_imports() -> Built {
    let mut body = Vec::new();
    write_u32(&mut body, 1);
    name(&mut body, "wbg");
    name(&mut body, "__wbg_log");
    body.extend([0x00, 0]);
    (2, body)
}

/// The program's read-only data at [`BASE`] as the linker merges it: `ok`, then `@{}/{}`, whose last
/// three bytes are also the template `/{}` (`format!("/{}", …)` compiles to it), then `!`.
const BASE: i32 = 0x400;
const DATA: &[u8] = b"ok@{}/{}!";

/// Its data symbols: name, offset and size.
const SYMBOLS: [(&str, u32, u32); 4] = [
    (".L.str.ok", 0, 2),
    (".L.str.route", 2, 6),
    (".L.str.template", 5, 3),
    (".L.str.bang", 8, 1),
];

/// The linking section of a program whose data symbols are `symbols` in segment 0, beside what the
/// repair reads past: a function symbol, a section symbol, an undefined data symbol and the
/// linker's absolute `__heap_base`, whose offset is an address past every segment's end.
fn linking(symbols: &[(&str, u32, u32)]) -> Built {
    let mut table = Vec::new();
    write_u32(&mut table, symbols.len() as u32 + 4);
    table.extend([0, 0x00, 0]);
    name(&mut table, "main");
    table.extend([3, 0x02, 7]);
    table.extend([1, 0x10]);
    name(&mut table, "elsewhere");
    table.extend([1, 0x84, 0x04]);
    name(&mut table, "__heap_base");
    table.extend([0, 0x80, 0x80, 0x04, 0]);
    for (called, offset, size) in symbols {
        table.extend([1, 0x00]);
        name(&mut table, called);
        write_u32(&mut table, 0);
        write_u32(&mut table, *offset);
        write_u32(&mut table, *size);
    }
    let mut contents = vec![2];
    // An init functions subsection first, which the repair skips.
    contents.extend([6, 1, 0]);
    contents.push(8);
    write_u32(&mut contents, table.len() as u32);
    contents.extend(table);
    custom("linking", &contents)
}

/// The program the linker made, with its relocations' linking section.
fn program_of(data: &[u8], symbols: &[(&str, u32, u32)]) -> Vec<u8> {
    module(&[
        memory(),
        (11, vector(&[active(BASE, data)])),
        linking(symbols),
    ])
}

fn program() -> Vec<u8> {
    program_of(DATA, &SYMBOLS)
}

/// The main module the splitter writes for `data`, after wasm-opt: the bytes of each symbol only a
/// chunk reaches (by name) zeroed, as `prune_main_symbols` zeroes them, then every run of zeros
/// left out of its segments, as wasm-opt's memory packing leaves them out.
fn split_main_of(data: &[u8], symbols: &[(&str, u32, u32)], chunks: &[&str]) -> Vec<u8> {
    let mut bytes = data.to_vec();
    for (called, offset, size) in symbols {
        if chunks.contains(called) {
            bytes[*offset as usize..(*offset + *size) as usize].fill(0);
        }
    }
    let mut segments = Vec::new();
    let mut start = None;
    for at in 0..=bytes.len() {
        match (start, bytes.get(at).copied().unwrap_or(0)) {
            (None, byte) if byte != 0 => start = Some(at),
            (Some(from), 0) => {
                segments.push(active(BASE + from as i32, &bytes[from..at]));
                start = None;
            }
            _ => {}
        }
    }
    let count = segments.len() as u32;
    let mut data_count = Vec::new();
    write_u32(&mut data_count, count);
    module(&[
        main_imports(),
        memory(),
        (12, data_count),
        (11, vector(&segments)),
    ])
}

/// The main module as dx writes it for this program: `.L.str.route` and `.L.str.bang` are a
/// chunk's, the others the main module's.
fn split_main() -> Vec<u8> {
    split_main_of(DATA, &SYMBOLS, &[".L.str.route", ".L.str.bang"])
}

fn linked() -> constants::Program {
    constants::Program::read(&program()).expect("the program")
}

/// The bytes `module`'s active data segments write at [`BASE`], as many as the program's data.
fn memory_of(module: &[u8]) -> Vec<u8> {
    let module = wasm::Module::read(module).unwrap();
    let mut seen = vec![0; DATA.len()];
    for segment in wasm::data_segments(module.section(wasm::DATA).unwrap().body()).unwrap() {
        let (
            wasm::Mode::Active {
                offset: Some(start),
                ..
            },
            wasm::Fill::Bytes(bytes),
        ) = (segment.mode, segment.fill)
        else {
            continue;
        };
        for (at, byte) in bytes.iter().enumerate() {
            if let Some(slot) = seen.get_mut((start - BASE) as usize + at) {
                *slot = *byte;
            }
        }
    }
    seen
}

fn unsafe_why(result: Result<impl Sized, Failure>) -> String {
    match result {
        Err(Failure::Unsafe(why)) => why,
        Err(Failure::Input(why)) => panic!("read as unreadable input: {why}"),
        Ok(_) => panic!("passed"),
    }
}

fn input_why(result: Result<impl Sized, Failure>) -> String {
    match result {
        Err(Failure::Input(why)) => why,
        Err(Failure::Unsafe(why)) => panic!("read as unsafe: {why}"),
        Ok(_) => panic!("passed"),
    }
}

// The merged constants.

#[test]
fn the_splitter_s_main_module_reads_a_merged_template_as_zeros_and_the_check_fails_it() {
    let main = split_main();
    // The template's three bytes read zero until the route's chunk loads.
    assert_eq!(memory_of(&main), b"ok\0\0\0\0\0\0\0");
    let why = unsafe_why(constants::check(&linked(), &main));
    assert_eq!(
        why,
        "the main module reads zero at 0x405, where .L.str.route and .L.str.template lie: a merged \
         constant the splitter zeroed for a chunk, which the main module may read"
    );
}

#[test]
fn the_repair_writes_the_merged_template_back_and_leaves_the_chunks_own_bytes_zero() {
    let main = split_main();
    let repaired = constants::repair(&linked(), &main).expect("repaired");
    assert_eq!((repaired.runs, repaired.written), (1, 3));
    // `/{}` reads whole again; `@{}` and `!`, which no symbol of the main module holds, stay zero.
    assert_eq!(memory_of(&repaired.bytes), b"ok\0\0\0/{}\0");
    constants::check(&linked(), &repaired.bytes).expect("the repaired module passes");
    // Written back after the module's own segments, which stay as they were, and counted.
    let module = wasm::Module::read(&repaired.bytes).unwrap();
    let segments = wasm::data_segments(module.section(wasm::DATA).unwrap().body()).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[1].raw, active(BASE + 5, b"/{}"));
    assert_eq!(module.section(wasm::DATA_COUNT).unwrap().body(), [2]);
    // A second repair finds nothing to write back.
    let again = constants::repair(&linked(), &repaired.bytes).unwrap();
    assert_eq!((again.runs, again.bytes), (0, repaired.bytes));
}

#[test]
fn a_kept_constant_whose_tail_a_chunk_s_constant_shares_reads_whole_after_the_repair() {
    // Here the route is the main module's and the template a chunk's: zeroing the template zeroes
    // the route's tail.
    let main = split_main_of(DATA, &SYMBOLS, &[".L.str.template", ".L.str.bang"]);
    assert_eq!(memory_of(&main), b"ok@{}\0\0\0\0");
    let repaired = constants::repair(&linked(), &main).unwrap();
    assert_eq!(memory_of(&repaired.bytes), b"ok@{}/{}\0");
}

#[test]
fn constants_that_overlap_in_part_are_written_back_where_they_overlap() {
    // `{}/` (2..5 of `@{}/{}` read from 3) lies over the route's middle and nothing else.
    let symbols = [
        (".L.str.ok", 0, 2),
        (".L.str.route", 2, 6),
        (".L.str.middle", 3, 3),
        (".L.str.bang", 8, 1),
    ];
    let program = constants::Program::read(&program_of(DATA, &symbols)).unwrap();
    let main = split_main_of(DATA, &symbols, &[".L.str.route"]);
    let repaired = constants::repair(&program, &main).unwrap();
    assert_eq!(memory_of(&repaired.bytes), b"ok\0{}/\0\0!");
    assert_eq!((repaired.runs, repaired.written), (1, 3));
}

#[test]
fn a_main_module_the_splitter_zeroed_nothing_in_is_kept_byte_for_byte() {
    let main = split_main_of(DATA, &SYMBOLS, &[]);
    let repaired = constants::repair(&linked(), &main).unwrap();
    assert_eq!((repaired.runs, repaired.written), (0, 0));
    assert_eq!(repaired.bytes, main);
}

#[test]
fn the_check_fails_a_zero_no_symbol_holds() {
    // The program's `!` has no symbol, and the main module reads zero there.
    let program = constants::Program::read(&program_of(DATA, &SYMBOLS[..3])).unwrap();
    let main = split_main_of(DATA, &SYMBOLS, &[".L.str.bang"]);
    assert_eq!(
        unsafe_why(constants::repair(&program, &main)),
        "the main module reads zero at 0x408, which no data symbol holds"
    );
}

#[test]
fn the_check_fails_a_symbol_zeroed_in_part() {
    // Two bytes of `ok`, which no other symbol shares, and only one of them zero.
    let main = split_main_of(b"o\0@{}/{}!", &SYMBOLS, &[]);
    assert_eq!(
        unsafe_why(constants::check(&linked(), &main)),
        "the main module reads zero over part of .L.str.ok at 0x400: the splitter zeroes a whole \
         symbol or none of it, and the repair rests on that"
    );
}

#[test]
fn the_check_fails_a_main_module_whose_data_is_not_the_program_s() {
    let main = split_main_of(b"OK@{}/{}!", &SYMBOLS, &[]);
    assert_eq!(
        unsafe_why(constants::repair(&linked(), &main)),
        "the main module holds 0x4f at 0x400, where the program holds 0x6f: it is not that \
         program's main module"
    );
}

#[test]
fn a_program_linked_without_its_relocations_is_refused_naming_emit_relocs() {
    let bare = module(&[memory(), (11, vector(&[active(BASE, DATA)]))]);
    let Err(why) = constants::Program::read(&bare) else {
        panic!("read")
    };
    assert_eq!(
        why,
        "it has no linking section: the program is linked with --emit-relocs, as dx links it for \
         --wasm-split"
    );
}

#[test]
fn a_main_module_with_a_passive_data_segment_is_not_read() {
    let main = module(&[
        memory(),
        (11, vector(&[active(BASE, b"ok"), passive(b"later")])),
    ]);
    assert_eq!(
        input_why(constants::repair(&linked(), &main)),
        "the main module's data segment 1 is not placed at a constant address of memory 0, which \
         the repair does not read"
    );
}

// The chunks' order.

/// A chunk: its data segments, then its element segments, each as encoded, in this order.
fn chunk(data: &[Vec<u8>], elements: &[Vec<u8>]) -> Vec<u8> {
    let mut data_count = Vec::new();
    write_u32(&mut data_count, data.len() as u32);
    module(&[
        chunk_imports(),
        (9, vector(elements)),
        (12, data_count),
        (11, vector(data)),
    ])
}

/// The data and table entries of one chunk: a symbol's bytes each, a function each.
fn chunk_data() -> Vec<Vec<u8>> {
    vec![
        active(0x500, b"tasks"),
        active(0x480, b"reports"),
        active(0x510, b"{}"),
    ]
}

fn chunk_entries() -> Vec<Vec<u8>> {
    vec![entries(9, &[3]), entries(4, &[1]), entries(7, &[2])]
}

/// What order a section's segments hold: each one's encoding.
fn segments_of(chunk: &[u8], id: u8) -> Vec<Vec<u8>> {
    let module = wasm::Module::read(chunk).unwrap();
    let body = module.section(id).unwrap().body();
    let segments = if id == wasm::DATA {
        wasm::data_segments(body)
    } else {
        wasm::element_segments(body)
    };
    segments
        .unwrap()
        .iter()
        .map(|segment| segment.raw.to_vec())
        .collect()
}

#[test]
fn two_splits_whose_segments_differ_in_order_are_the_same_bytes_once_ordered() {
    // As two runs of the splitter write one chunk: the same segments, in two hash maps' orders.
    let (data, elements) = (chunk_data(), chunk_entries());
    let first = chunk(&data, &elements);
    let second = chunk(
        &[data[2].clone(), data[0].clone(), data[1].clone()],
        &[
            elements[1].clone(),
            elements[2].clone(),
            elements[0].clone(),
        ],
    );
    assert_ne!(first, second);
    let (one, two) = (
        order::order(&first).unwrap(),
        order::order(&second).unwrap(),
    );
    assert_eq!(one.bytes, two.bytes);
    assert_eq!(one.bytes.len(), first.len());
    // In the order of their addresses.
    assert_eq!(
        segments_of(&one.bytes, wasm::DATA),
        [data[1].clone(), data[0].clone(), data[2].clone()]
    );
    assert_eq!(
        segments_of(&one.bytes, wasm::ELEMENT),
        [
            elements[1].clone(),
            elements[2].clone(),
            elements[0].clone()
        ]
    );
    assert_eq!((one.moved, one.segments), (5, 6));
    assert_eq!((two.moved, two.segments), (2, 6));
}

#[test]
fn a_chunk_in_order_already_is_kept_byte_for_byte() {
    let ordered = order::order(&chunk(&chunk_data(), &chunk_entries())).unwrap();
    let again = order::order(&ordered.bytes).unwrap();
    assert_eq!((again.moved, again.segments), (0, 6));
    assert_eq!(again.bytes, ordered.bytes);
}

#[test]
fn a_passive_segment_keeps_its_index_while_the_active_ones_take_their_order() {
    let data = [active(0x500, b"b"), passive(b"later"), active(0x400, b"a")];
    let ordered = order::order(&chunk(&data, &[])).unwrap();
    assert_eq!(
        segments_of(&ordered.bytes, wasm::DATA),
        [data[2].clone(), data[1].clone(), data[0].clone()]
    );
}

#[test]
fn segments_that_fill_one_place_alike_are_ordered_and_otherwise_refused() {
    // Two symbols the linker merged, each written whole by the chunk: the same bytes where they meet.
    let alike = [active(0x405, b"/{}"), active(0x402, b"@{}/{}")];
    let ordered = order::order(&chunk(&alike, &[])).unwrap();
    assert_eq!(
        segments_of(&ordered.bytes, wasm::DATA),
        [alike[1].clone(), alike[0].clone()]
    );
    let unlike = [active(0x405, b"/{}"), active(0x402, b"@{}/()")];
    assert_eq!(
        unsafe_why(order::order(&chunk(&unlike, &[]))),
        "its data segments 1 and 0 fill 0x406 with different contents, so their order matters"
    );
    let entries = [entries(4, &[1, 2]), entries(5, &[3])];
    assert_eq!(
        unsafe_why(order::order(&chunk(&[], &entries))),
        "its element segments 0 and 1 fill 0x5 with different contents, so their order matters"
    );
}

#[test]
fn a_segment_whose_address_is_not_a_constant_is_refused() {
    // Placed at `global.get 0`.
    let placed = vec![0, 0x23, 0, 0x0b, 1, b'x'];
    assert_eq!(
        unsafe_why(order::order(&chunk(&[active(0x400, b"a"), placed], &[]))),
        "its data segment 1 has no constant address: whether its order matters cannot be told"
    );
}

#[test]
fn a_chunk_whose_name_section_names_its_segments_is_refused_once_they_move() {
    let data = chunk_data();
    let mut named = chunk(&data, &[]);
    // A name section naming data segment 0.
    let mut names = vec![9];
    let mut map = Vec::new();
    write_u32(&mut map, 1);
    write_u32(&mut map, 0);
    name(&mut map, ".rodata.tasks");
    write_u32(&mut names, map.len() as u32);
    names.extend(map);
    let (id, body) = custom("name", &names);
    named.push(id);
    write_u32(&mut named, body.len() as u32);
    named.extend(body);
    assert_eq!(
        unsafe_why(order::order(&named)),
        "its name section names its segments, which a new order of them would leave naming others"
    );
}

// The output folder.

/// The shared chunk the splitter writes empty, as wasm-opt leaves it: no import, no section but
/// its custom ones.
fn empty_chunk() -> Vec<u8> {
    module(&[custom("producers", b"\0")])
}

/// What dx writes for one split build: the main module, the empty shared chunk, the page's chunk,
/// the script naming the three by dx's names, and the page naming the script by its own.
fn dx_output(tree: &Tree, folder: &str, main: &[u8], chunk_bytes: &[u8], dx: &str) {
    let assets = tree.path(&format!("{folder}/assets"));
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("symphony-ui_bg-dxh1a2b.wasm"), main).unwrap();
    std::fs::write(assets.join("chunk_0_split-dxhc0.wasm"), empty_chunk()).unwrap();
    std::fs::write(
        assets.join(format!("module_0_TasksKind-dxh{dx}.wasm")),
        chunk_bytes,
    )
    .unwrap();
    tree.write(
        &format!("{folder}/assets/symphony-ui-dxh{dx}.js"),
        &format!(
            "const c=makeLoad(\"/./assets/chunk_0_split-dxhc0.wasm\",[],f);\
             const a=makeLoad(\"/./assets/module_0_TasksKind-dxh{dx}.wasm\",[c],f);\
             __wbg_init({{module_or_path:\"/./assets/symphony-ui_bg-dxh1a2b.wasm\"}}).then(w=>w);"
        ),
    );
    tree.write(&format!("{folder}/assets/board-dxh77.css"), "main{}");
    tree.write(
        &format!("{folder}/index.html"),
        &format!(
            "<link rel=\"preload\" as=\"script\" href=\"/./assets/symphony-ui-dxh{dx}.js\">\
             <script type=\"module\" async src=\"/./assets/symphony-ui-dxh{dx}.js\"></script>"
        ),
    );
}

fn run(args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// Every file under `folder`, by its path there, with its bytes.
fn files(tree: &Tree, folder: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for name in tree.names(&format!("{folder}/assets")) {
        let bytes = std::fs::read(tree.path(&format!("{folder}/assets/{name}"))).unwrap();
        files.push((format!("assets/{name}"), bytes));
    }
    let page = std::fs::read(tree.path(&format!("{folder}/index.html"))).unwrap();
    files.push(("index.html".into(), page));
    files
}

/// The file whose name under `assets/` starts with `stem`, in `files`.
fn file<'a>(files: &'a [(String, Vec<u8>)], stem: &str) -> &'a (String, Vec<u8>) {
    files
        .iter()
        .find(|(name, _)| name.starts_with(&format!("assets/{stem}")))
        .unwrap_or_else(|| panic!("no {stem} in {files:?}"))
}

/// A program and two outputs of it, `one` and `two`, as two builds write them: the chunk's segments
/// in two orders, so two names for it from dx, and so two scripts and two names for those.
fn two_builds(tree: &Tree) -> String {
    let path = tree.path("program.wasm");
    std::fs::write(&path, program()).unwrap();
    let (data, elements) = (chunk_data(), chunk_entries());
    let reordered = chunk(
        &[data[2].clone(), data[0].clone(), data[1].clone()],
        &[
            elements[2].clone(),
            elements[0].clone(),
            elements[1].clone(),
        ],
    );
    dx_output(tree, "one", &split_main(), &chunk(&data, &elements), "aa11");
    dx_output(tree, "two", &split_main(), &reordered, "bb22");
    path.to_str().unwrap().to_string()
}

#[test]
fn two_builds_differing_in_chunk_order_and_dx_s_names_are_the_same_files_once_repaired() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    assert_ne!(files(&tree, "one"), files(&tree, "two"));
    let (code, report, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!((code, err.as_str()), (0, ""), "{report}");
    let (code, _, err) = run(&[&program, tree.path("two").to_str().unwrap()]);
    assert_eq!((code, err.as_str()), (0, ""));
    let repaired = files(&tree, "one");
    assert_eq!(repaired, files(&tree, "two"));

    // Each module and the script named by its bytes; the style sheet left as it was.
    let (main, empty, chunk, script) = (
        file(&repaired, "symphony-ui_bg-"),
        file(&repaired, "chunk_0_split-"),
        file(&repaired, "module_0_TasksKind-"),
        file(&repaired, "symphony-ui-"),
    );
    assert_eq!(repaired.len(), 6, "{repaired:?}");
    assert_eq!(file(&repaired, "board-dxh77.css").1, b"main{}");
    assert_eq!(empty.1, empty_chunk());
    for (name, bytes) in [main, empty, chunk, script] {
        let name = name.strip_prefix("assets/").unwrap();
        assert_eq!(name, named(name, bytes));
        assert!(!name.contains("-dxh"), "{name}");
    }
    // The script names the modules so, and the page the script.
    let text = String::from_utf8(script.1.clone()).unwrap();
    assert_eq!(
        text,
        format!(
            "const c=makeLoad(\"/./{}\",[],f);const a=makeLoad(\"/./{}\",[c],f);\
             __wbg_init({{module_or_path:\"/./{}\"}}).then(w=>w);",
            empty.0, chunk.0, main.0
        )
    );
    let page = String::from_utf8(repaired.last().unwrap().1.clone()).unwrap();
    assert_eq!(
        page.matches(&format!("\"/./{}\"", script.0)).count(),
        2,
        "{page}"
    );
    // The main module reads the template again; the chunk's segments are in order.
    assert_eq!(memory_of(&main.1), b"ok\0\0\0/{}\0");
    let data = chunk_data();
    assert_eq!(
        segments_of(&chunk.1, wasm::DATA),
        [data[1].clone(), data[0].clone(), data[2].clone()]
    );
    let short = |file: &(String, Vec<u8>)| file.0["assets/".len()..].to_string();
    assert_eq!(
        report,
        format!(
            "main module symphony-ui_bg-dxh1a2b.wasm -> {}: 1 merged constants written back, 3 \
             bytes\nchunk chunk_0_split-dxhc0.wasm -> {}: 0 of its 0 segments put in order\n\
             chunk module_0_TasksKind-dxhaa11.wasm -> {}: 5 of its 6 segments put in order\n\
             script symphony-ui-dxhaa11.js -> {}\n",
            short(main),
            short(empty),
            short(chunk),
            short(script)
        )
    );
}

#[test]
fn a_second_run_changes_nothing() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    let public = tree.path("one");
    assert_eq!(run(&[&program, public.to_str().unwrap()]).0, 0);
    let first = files(&tree, "one");
    let (code, report, err) = run(&[&program, public.to_str().unwrap()]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(files(&tree, "one"), first);
    let short = |stem| file(&first, stem).0["assets/".len()..].to_string();
    assert_eq!(
        report,
        format!(
            "main module {}: 0 merged constants written back, 0 bytes\n\
             chunk {}: 0 of its 0 segments put in order\n\
             chunk {}: 0 of its 6 segments put in order\n",
            short("symphony-ui_bg-"),
            short("chunk_0_split-"),
            short("module_0_TasksKind-")
        )
    );
}

#[test]
fn a_failed_check_writes_nothing_and_exits_1() {
    let tree = Tree::new();
    two_builds(&tree);
    // Another program: its data is not what the main module holds.
    let other = tree.path("other.wasm");
    std::fs::write(&other, program_of(b"OK@{}/{}!", &SYMBOLS)).unwrap();
    let before = files(&tree, "one");
    let (code, out, err) = run(&[other.to_str().unwrap(), tree.path("one").to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "symphony-build split-repair: symphony-ui_bg-dxh1a2b.wasm: the main module holds 0x6f at \
         0x400, where the program holds 0x4f: it is not that program's main module\n"
    );
    assert_eq!(files(&tree, "one"), before);
}

#[test]
fn an_output_without_a_chunk_or_with_two_main_modules_is_refused_and_left_as_it_was() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    let assets = tree.path("one/assets");
    std::fs::remove_file(assets.join("module_0_TasksKind-dxhaa11.wasm")).unwrap();
    std::fs::remove_file(assets.join("chunk_0_split-dxhc0.wasm")).unwrap();
    let before = files(&tree, "one");
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        format!(
            "symphony-build split-repair: {}: no chunk, a module that defines no memory: it is not the \
             output of dx bundle --wasm-split\n",
            assets.display()
        )
    );
    assert_eq!(files(&tree, "one"), before);
    std::fs::write(assets.join("another_bg-dxh9.wasm"), split_main()).unwrap();
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        err.ends_with(
            ": another_bg-dxh9.wasm and symphony-ui_bg-dxh1a2b.wasm both define a memory, where \
             one main module is expected\n"
        ),
        "{err}"
    );
}

#[test]
fn a_module_no_script_names_is_refused() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    tree.write(
        "one/assets/symphony-ui-dxhaa11.js",
        "makeLoad(\"/./assets/elsewhere.wasm\")",
    );
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build split-repair: symphony-ui_bg-dxh1a2b.wasm: no script in "),
        "{err}"
    );
}

#[test]
fn a_script_the_page_does_not_name_is_refused() {
    let tree = Tree::new();
    let program = two_builds(&tree);
    tree.write("one/index.html", "<p>no script</p>");
    let (code, _, err) = run(&[&program, tree.path("one").to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "symphony-build split-repair: symphony-ui-dxhaa11.js: index.html does not name it\n"
    );
}

#[test]
fn a_command_line_without_its_two_paths_is_its_usage_and_exit_2() {
    for args in [&[][..], &["program.wasm"], &["a", "b", "c"]] {
        assert_eq!(
            run(args),
            (2, String::new(), format!("{USAGE}\n")),
            "{args:?}"
        );
    }
    let (code, _, err) = run(&["/nowhere/program.wasm", "/nowhere/public"]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("symphony-build split-repair: /nowhere/program.wasm: "),
        "{err}"
    );
}

// The format.

#[test]
fn numbers_read_to_their_bounds_and_no_further() {
    let unsigned = |bytes: &[u8]| wasm::Reader::new(bytes).u32();
    let signed = |bytes: &[u8]| wasm::Reader::new(bytes).i32();
    assert_eq!(unsigned(&[0xff, 0xff, 0xff, 0xff, 0x0f]), Ok(u32::MAX));
    assert_eq!(unsigned(&[0xe5, 0x8e, 0x26]), Ok(624_485));
    for beyond in [
        &[0xff, 0xff, 0xff, 0xff, 0x1f][..],
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0],
    ] {
        assert_eq!(
            unsigned(beyond),
            Err("the number at byte 0 is too large".into())
        );
    }
    assert_eq!(signed(&[0x80, 0x80, 0x80, 0x80, 0x78]), Ok(i32::MIN));
    assert_eq!(signed(&[0xff, 0xff, 0xff, 0xff, 0x07]), Ok(i32::MAX));
    assert_eq!(signed(&[0x7f]), Ok(-1));
    assert_eq!(
        signed(&[0x80, 0x80, 0x80, 0x80, 0x08]),
        Err("the number at byte 0 is too large".into())
    );
    assert_eq!(
        unsigned(&[0x80]),
        Err("it ends at byte 1 in the middle of a field".into())
    );
    for value in [0, 1, 63, 64, -64, -65, 0x400, i32::MAX, i32::MIN] {
        let mut out = Vec::new();
        write_i32(&mut out, value);
        let mut reader = wasm::Reader::new(&out);
        assert_eq!((reader.i32(), reader.done()), (Ok(value), true), "{value}");
    }
    for value in [0, 127, 128, u32::MAX] {
        let mut out = Vec::new();
        write_u32(&mut out, value);
        assert_eq!(unsigned(&out), Ok(value), "{value}");
    }
    let minus_one = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
    assert_eq!(wasm::Reader::new(&minus_one).i64(), Ok(-1));
}

#[test]
fn a_module_that_is_none_or_ends_early_is_refused() {
    let read = |bytes: &[u8]| wasm::Module::read(bytes).err();
    assert_eq!(
        read(b"\0asm\x02\0\0\0"),
        Some("it is not a WebAssembly module of version 1".into())
    );
    assert_eq!(
        read(b"\0asm\x01\0\0\0\x0b\x05\x01"),
        Some("a field of 5 bytes at byte 2 runs past its end".into())
    );
    // A section left as it was keeps the length as its writer encoded it, padded or not.
    let padded = b"\0asm\x01\0\0\0\x05\x83\x80\x80\x80\x00\x01\x00\x01".to_vec();
    assert_eq!(wasm::Module::read(&padded).unwrap().write(), padded);
    // A vector counting more items than its bytes could hold.
    assert_eq!(
        wasm::data_segments(&[0x05, 0x01]).err(),
        Some("the 5 items counted at byte 0 cannot fit in the bytes left".into())
    );
}

#[test]
fn segments_of_every_form_are_read_with_their_place_and_contents() {
    let mut body = Vec::new();
    write_u32(&mut body, 7);
    // Passive, by function index; active in table 2; declared; then by expression: active in
    // table 0, passive, active in table 1, declared.
    body.extend([1, 0x00, 1, 5]);
    body.extend([2, 2, 0x41, 3, 0x0b, 0x00, 2, 6, 7]);
    body.extend([3, 0x00, 1, 8]);
    body.extend([4, 0x41, 1, 0x0b, 2, 0xd2, 9, 0x0b, 0xd0, 0x70, 0x0b]);
    body.extend([5, 0x70, 1, 0xd2, 10, 0x0b]);
    body.extend([6, 1, 0x23, 0, 0x0b, 0x70, 0]);
    body.extend([7, 0x70, 1, 0xd2, 11, 0x0b]);
    let segments = wasm::element_segments(&body).unwrap();
    let places: Vec<(wasm::Mode, usize)> = segments
        .iter()
        .map(|segment| (segment.mode, segment.fill.len()))
        .collect();
    use wasm::Mode::{Active, Declared, Passive};
    assert_eq!(
        places,
        [
            (Passive, 1),
            (
                Active {
                    index: 2,
                    offset: Some(3)
                },
                2
            ),
            (Declared, 1),
            (
                Active {
                    index: 0,
                    offset: Some(1)
                },
                2
            ),
            (Passive, 1),
            (
                Active {
                    index: 1,
                    offset: None
                },
                0
            ),
            (Declared, 1),
        ]
    );
    let raw: Vec<u8> = segments
        .iter()
        .flat_map(|segment| segment.raw.to_vec())
        .collect();
    assert_eq!(raw, body[1..]);
    // A data segment in memory 1, and one of unknown flags.
    let data = wasm::data_segments(&[1, 2, 1, 0x41, 0x7f, 0x0b, 1, 9]).unwrap();
    assert_eq!(
        data[0].mode,
        Active {
            index: 1,
            offset: Some(-1)
        }
    );
    assert_eq!(
        wasm::data_segments(&[1, 3]).err(),
        Some("a data segment with unknown flags 3".into())
    );
    assert_eq!(
        wasm::element_segments(&[1, 0x00, 0x2a]).err(),
        Some("a constant expression at byte 2 holds an instruction it does not read, 0x2a".into())
    );
    assert_eq!(
        wasm::element_segments(&[1, 8]).err(),
        Some("an element segment with unknown flags 8".into())
    );
}

#[test]
fn a_module_defines_a_memory_when_its_memory_section_holds_one() {
    let defines = |sections: &[Built]| {
        wasm::Module::read(&module(sections))
            .unwrap()
            .defines_memory()
            .unwrap()
    };
    assert!(defines(&[memory()]));
    assert!(!defines(&[(5, vec![0])]));
    assert!(!defines(&[chunk_imports()]));
    assert!(!defines(&[custom("producers", b"")]));
}

// Names.

#[test]
fn a_file_is_named_by_its_stem_and_its_bytes_whatever_hash_it_was_named_by() {
    // The first 16 hex digits of the SHA-256 of `split`.
    let hash = "ad1a64057f9ab34f";
    for name in [
        "module_1_TasksKind-dxh7793f3478548eae.wasm",
        "module_1_TasksKind-ad1a64057f9ab34f.wasm",
        "module_1_TasksKind-0000000000000000.wasm",
        "module_1_TasksKind.wasm",
    ] {
        assert_eq!(
            named(name, b"split"),
            format!("module_1_TasksKind-{hash}.wasm"),
            "{name}"
        );
    }
    // A last part that is no hash stays part of the stem.
    assert_eq!(
        named("symphony-ui_bg.wasm", b"split"),
        format!("symphony-ui_bg-{hash}.wasm")
    );
    assert_eq!(named("a-dxh.wasm", b"split"), format!("a-dxh-{hash}.wasm"));
    assert_eq!(named("a-dxhzz.js", b"split"), format!("a-dxhzz-{hash}.js"));
    assert_eq!(named("bare", b"split"), format!("bare-{hash}"));
}

#[test]
fn only_a_whole_file_name_is_renamed() {
    let mut text = b"\"/a/x.wasm\" x.wasm.map ax.wasm x.wasm-2 'x.wasm'".to_vec();
    assert_eq!(rename_in(&mut text, "x.wasm", "y.wasm"), 2);
    assert_eq!(text, b"\"/a/y.wasm\" x.wasm.map ax.wasm x.wasm-2 'y.wasm'");
    assert_eq!(mentions(&text, "y.wasm"), 2);
    assert_eq!(rename_in(&mut text, "", "z"), 0);
    let mut whole = b"x.wasm".to_vec();
    assert_eq!(rename_in(&mut whole, "x.wasm", "y.wasm"), 1);
    assert_eq!(whole, b"y.wasm");
}
